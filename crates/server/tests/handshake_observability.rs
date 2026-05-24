use std::sync::Arc;

use anyhow::Result;
use protocol::{
    ClientMessage, DisconnectReason, PROTOCOL_VERSION, ResourceAnnouncement,
    ResourceAvailabilityEntry, ResourceAvailabilityReport, ResourceAvailabilityStatus,
    ServerMessage, current_login_capabilities, decode_server_line, encode_line,
};
use server::{
    HeartbeatSection, ServerConfig, ServerRuntimeStatus, ServerSection, SharedState,
    run_with_listener_and_state,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

fn server_config(bind_addr: &str, motd: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: bind_addr.to_string(),
            tick_rate: 20,
            motd: motd.to_string(),
            ..ServerSection::default()
        },
        heartbeat: HeartbeatSection {
            server_ping_interval_ms: 0,
            ..HeartbeatSection::default()
        },
        ..ServerConfig::default()
    }
}

#[tokio::test]
async fn full_handshake_creates_session_and_reaches_ready_dry_run() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "test motd");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config.clone(), state,
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
                capabilities: current_login_capabilities(),
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
    let announcement = read_until_announcement(&mut lines).await?;
    assert_eq!(announcement.resources.len(), 1);
    assert_eq!(announcement.resources[0].name, "chat");

    // Send availability report immediately
    let report = build_available_report(&announcement);
    writer_half
        .write_all(
            encode_line(&ClientMessage::ResourceAvailabilityReport(report))?
                .as_bytes(),
        )
        .await?;

    // Read messages until JoinGateDecision
    let mut saw_chat_join = false;
    let gate = loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(d) => break d,
            ServerMessage::ServerPing { sequence } => {
                writer_half
                    .write_all(encode_line(&ClientMessage::ServerPong { sequence })?.as_bytes())
                    .await?;
            }
            ServerMessage::Pong { .. } => {}
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
    assert_eq!(entry.protocol_version, Some(PROTOCOL_VERSION));
    assert_eq!(entry.login_capabilities, Some(current_login_capabilities()));

    // --- Runtime status snapshot matches registry ---
    let status = ServerRuntimeStatus::from_config(&config).with_session_counts(
        reg_snap.connected_sessions,
        reg_snap.ready_dry_run_sessions,
        reg_snap.failed_sessions,
    );
    let status_text = status.to_text();
    assert!(status_text.contains("connected_sessions: 1"), "status: {status_text}");
    assert!(status_text.contains("ready_dry_run_sessions: 1"), "status: {status_text}");
    assert!(status_text.contains("failed_sessions: 0"), "status: {status_text}");
    assert!(status_text.contains("server_name: MeowV Local Dev Server"));
    assert!(status_text.contains("protocol_version: 2"));
    assert!(status_text.contains("diagnostics_enabled: true"));
    assert!(!status_text.contains("client_ip"));

    // --- Diagnostics to_text output (from unit tests) produces same structure ---
    let diag_text = status_text;
    assert!(diag_text.contains("connected_sessions: "));

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

    // --- Status after cleanup reflects zero sessions ---
    let status_after = ServerRuntimeStatus::from_config(&config)
        .with_session_counts(0, 0, 0);
    let status_after_text = status_after.to_text();
    assert!(status_after_text.contains("connected_sessions: 0"));
    assert!(status_after_text.contains("ready_dry_run_sessions: 0"));

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn version_mismatch_disconnects_and_cleans_up_session() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "test");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config, state,
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
                capabilities: current_login_capabilities(),
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
    let config = server_config(&addr.to_string(), "test");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config, state,
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
async fn missing_login_capability_payload_rejected() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string(), "test");

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            b"{\"type\":\"login\",\"name\":\"legacy\",\"protocol_version\":2}\n",
        )
        .await?;

    let disconnect = read_packet(&mut lines).await?;
    match disconnect {
        ServerMessage::Disconnect { reason, message } => {
            assert_eq!(reason, DisconnectReason::InvalidHandshake);
            assert!(message.contains("capabilities"));
        }
        other => panic!("expected Disconnect, got {other:?}"),
    }

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn registry_session_id_is_deterministic() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "test motd");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config, state,
    ));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "bob".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let welcome = read_packet(&mut lines).await?;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }));

    let announcement = read_until_announcement(&mut lines).await?;

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
            ServerMessage::ServerPing { sequence } => {
                writer_half
                    .write_all(encode_line(&ClientMessage::ServerPong { sequence })?.as_bytes())
                    .await?;
            }
            ServerMessage::Pong { .. } => {}
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
    assert_eq!(snap.sessions[0].protocol_version, Some(PROTOCOL_VERSION));
    assert_eq!(snap.sessions[0].login_capabilities, Some(current_login_capabilities()));

    drop(lines);
    drop(writer_half);
    tokio::time::sleep(Duration::from_millis(100)).await;

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn session_created_on_connect_before_login() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "test");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config, state,
    ));

    let _stream = TcpStream::connect(addr).await?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Handler creates session on connect before reading first message
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(format!("{:?}", snap.sessions[0].state), "Connected");
    assert_eq!(snap.sessions[0].protocol_version, None);

    // Cleanup: drop stream, session removed
    drop(_stream);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        server_state.registry.lock().unwrap().snapshot().connected_sessions,
        0
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn session_cleaned_up_on_early_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "test");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config, state,
    ));

    let stream = TcpStream::connect(addr).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Session was created
    assert_eq!(
        server_state.registry.lock().unwrap().snapshot().connected_sessions,
        1
    );

    // Close connection without sending any message
    drop(stream);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Registry cleaned up via SessionGuard
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0);
    assert_eq!(snap.failed_sessions, 0);
    assert_eq!(snap.ready_dry_run_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn runtime_status_reflects_live_session_counts() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = server_config(&addr.to_string(), "status test");

    let server_task = tokio::spawn(run_with_listener_and_state(
        listener, config.clone(), state,
    ));

    // Connect first client, let it reach ReadyDryRun
    let stream1 = TcpStream::connect(addr).await?;
    let (r1, mut w1) = stream1.into_split();
    let mut l1 = BufReader::new(r1).lines();

    w1.write_all(
        encode_line(&ClientMessage::Login {
            name: "alice".to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: current_login_capabilities(),
        })?
        .as_bytes(),
    )
    .await?;

    let _welcome = read_packet(&mut l1).await?;
    let announcement = read_until_announcement(&mut l1).await?;
    let report = build_available_report(&announcement);
    w1.write_all(
        encode_line(&ClientMessage::ResourceAvailabilityReport(report))?
            .as_bytes(),
    )
    .await?;

    let mut saw_chat = false;
    loop {
        let msg = read_packet(&mut l1).await?;
        match msg {
            ServerMessage::JoinGateDecision(_) => break,
            ServerMessage::ServerPing { sequence } => {
                w1.write_all(encode_line(&ClientMessage::ServerPong { sequence })?.as_bytes())
                    .await?;
            }
            ServerMessage::Pong { .. } => {}
            ServerMessage::ChatBroadcast { from, message } => {
                if from == "server" && message == "alice joined" {
                    saw_chat = true;
                }
            }
            ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert!(saw_chat);

    // Verify status from registry matches
    let snap = server_state.registry.lock().unwrap().snapshot();
    let status = ServerRuntimeStatus::from_config(&config).with_session_counts(
        snap.connected_sessions,
        snap.ready_dry_run_sessions,
        snap.failed_sessions,
    );
    assert_eq!(status.connected_sessions, 1);
    assert_eq!(status.ready_dry_run_sessions, 1);
    assert_eq!(status.failed_sessions, 0);
    assert!(status.to_text().contains("connected_sessions: 1"));
    assert!(status.to_text().contains("ready_dry_run_sessions: 1"));
    assert!(status.to_text().contains("failed_sessions: 0"));

    // Second client connects but doesn't complete handshake
    let _stream2 = TcpStream::connect(addr).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let snap2 = server_state.registry.lock().unwrap().snapshot();
    let status2 = ServerRuntimeStatus::from_config(&config).with_session_counts(
        snap2.connected_sessions,
        snap2.ready_dry_run_sessions,
        snap2.failed_sessions,
    );
    assert_eq!(status2.connected_sessions, 2);
    assert_eq!(status2.ready_dry_run_sessions, 1);
    assert_eq!(status2.failed_sessions, 0);
    assert!(status2.to_text().contains("connected_sessions: 2"));
    assert!(status2.to_text().contains("ready_dry_run_sessions: 1"));

    drop(w1);
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

async fn read_until_announcement<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
) -> Result<ResourceAnnouncement>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        match read_packet(lines).await? {
            ServerMessage::ResourceAnnouncement(announcement) => return Ok(announcement),
            ServerMessage::ChatBroadcast { .. }
            | ServerMessage::EntitySnapshot { .. }
            | ServerMessage::ServerPing { .. }
            | ServerMessage::Pong { .. } => continue,
            other => panic!("expected ResourceAnnouncement, got {other:?}"),
        }
    }
}
