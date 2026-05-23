use std::sync::Arc;

use anyhow::Result;
use protocol::{
    ClientMessage, DisconnectReason, PROTOCOL_VERSION, ResourceAnnouncement,
    ResourceAvailabilityEntry, ResourceAvailabilityReport, ResourceAvailabilityStatus,
    ServerMessage, decode_server_line, encode_line,
};
use server::{ServerConfig, ServerSection, SharedState, run_with_listener_and_state};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

#[tokio::test]
async fn full_handshake_creates_session_and_reaches_ready_dry_run() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener,
        ServerConfig {
            server: ServerSection {
                bind_addr: addr.to_string(),
                tick_rate: 20,
                motd: "test motd".to_string(),
                ..ServerSection::default()
            },
            ..ServerConfig::default()
        },
        state,
    ));

    assert_eq!(
        server_state.registry.lock().unwrap().snapshot().connected_sessions,
        0
    );

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // --- Login ---
    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION,
            })?
            .as_bytes(),
        )
        .await?;

    // Expect Welcome
    let welcome = read_packet(&mut lines).await?;
    match &welcome {
        ServerMessage::Welcome { motd, protocol_version, .. } => {
            assert_eq!(motd, "test motd");
            assert_eq!(*protocol_version, PROTOCOL_VERSION);
        }
        other => panic!("expected Welcome, got {other:?}"),
    }

    // Expect ResourceAnnouncement
    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };
    assert_eq!(announcement.resources.len(), 1);
    assert_eq!(announcement.resources[0].name, "chat");

    // Send availability report immediately (don't need to wait for chat broadcast)
    let report = build_available_report(&announcement);
    writer_half
        .write_all(
            encode_line(&ClientMessage::ResourceAvailabilityReport(report))?
                .as_bytes(),
        )
        .await?;

    // Expect ChatBroadcast "alice joined" may arrive before or interleaved
    // Read messages until we see JoinGateDecision
    let mut saw_chat_join = false;
    let gate = loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(d) => break d,
            ServerMessage::ChatBroadcast { from, message } => {
                if from == "server" && message == "alice joined" {
                    saw_chat_join = true;
                }
            }
            ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected message during handshake: {other:?}"),
        }
    };
    assert!(saw_chat_join, "should have received 'alice joined' broadcast");
    assert_eq!(
        format!("{:?}", gate.outcome),
        "WouldAllow",
        "expected WouldAllow with all resources available"
    );

    // --- Registry verification ---
    let reg_snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(reg_snap.connected_sessions, 1);
    assert_eq!(reg_snap.ready_dry_run_sessions, 1);
    assert_eq!(reg_snap.failed_sessions, 0);
    assert_eq!(reg_snap.sessions.len(), 1);

    let entry = &reg_snap.sessions[0];
    assert!(entry.ready_dry_run);
    assert!(!entry.failed);
    assert_eq!(entry.event_count, 11);

    // --- Cleanup: disconnect, session removed ---
    drop(lines);
    drop(writer_half);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        server_state
            .registry
            .lock()
            .unwrap()
            .snapshot()
            .connected_sessions,
        0
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn version_mismatch_disconnects_and_cleans_up_session() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener,
        ServerConfig {
            server: ServerSection {
                bind_addr: addr.to_string(),
                tick_rate: 20,
                motd: "test".to_string(),
                ..ServerSection::default()
            },
            ..ServerConfig::default()
        },
        state,
    ));

    assert_eq!(
        server_state.registry.lock().unwrap().snapshot().connected_sessions,
        0
    );

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION + 1,
            })?
            .as_bytes(),
        )
        .await?;

    let disconnect = read_packet(&mut lines).await?;
    match disconnect {
        ServerMessage::Disconnect { reason, message } => {
            assert_eq!(reason, DisconnectReason::ProtocolMismatch);
            assert!(message.contains("protocol mismatch"));
        }
        other => panic!("expected Disconnect, got {other:?}"),
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0);
    assert_eq!(snap.failed_sessions, 0);
    assert_eq!(snap.ready_dry_run_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn invalid_handshake_first_message_not_login() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener,
        ServerConfig {
            server: ServerSection {
                bind_addr: addr.to_string(),
                tick_rate: 20,
                motd: "test".to_string(),
                ..ServerSection::default()
            },
            ..ServerConfig::default()
        },
        state,
    ));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Chat {
                message: "hello".to_string(),
            })?
            .as_bytes(),
        )
        .await?;

    let disconnect = read_packet(&mut lines).await?;
    match disconnect {
        ServerMessage::Disconnect { reason, message } => {
            assert_eq!(reason, DisconnectReason::InvalidHandshake);
            assert!(message.contains("first packet must be login"));
        }
        other => panic!("expected Disconnect, got {other:?}"),
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn registry_session_id_is_deterministic() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener,
        ServerConfig {
            server: ServerSection {
                bind_addr: addr.to_string(),
                tick_rate: 20,
                motd: "test motd".to_string(),
                ..ServerSection::default()
            },
            ..ServerConfig::default()
        },
        state,
    ));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "bob".to_string(),
                protocol_version: PROTOCOL_VERSION,
            })?
            .as_bytes(),
        )
        .await?;

    let welcome = read_packet(&mut lines).await?;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }));

    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    let report = build_available_report(&announcement);
    writer_half
        .write_all(
            encode_line(&ClientMessage::ResourceAvailabilityReport(report))?
                .as_bytes(),
        )
        .await?;

    let mut saw_chat_join = false;
    let gate = loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(d) => break d,
            ServerMessage::ChatBroadcast { from, message } => {
                if from == "server" && message == "bob joined" {
                    saw_chat_join = true;
                }
            }
            ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected message: {other:?}"),
        }
    };
    assert!(saw_chat_join);
    assert_eq!(format!("{:?}", gate.outcome), "WouldAllow");

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].id.to_string(), "session-1");
    assert_eq!(snap.sessions[0].event_count, 11);

    drop(lines);
    drop(writer_half);
    tokio::time::sleep(Duration::from_millis(100)).await;

    server_task.abort();
    Ok(())
}

fn build_available_report(announcement: &ResourceAnnouncement) -> ResourceAvailabilityReport {
    let resources = announcement
        .resources
        .iter()
        .flat_map(|r| {
            r.files.iter().map(|f| ResourceAvailabilityEntry {
                resource_name: r.name.clone(),
                file_path: f.relative_path.clone(),
                status: ResourceAvailabilityStatus::Available,
            })
        })
        .collect();
    ResourceAvailabilityReport {
        resources,
        is_fully_available: true,
    }
}

async fn read_packet<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<ServerMessage>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = timeout(Duration::from_secs(2), lines.next_line())
        .await??
        .expect("stream closed before packet arrived");
    Ok(decode_server_line(&line)?)
}
