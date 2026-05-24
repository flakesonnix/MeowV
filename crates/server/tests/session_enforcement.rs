use std::sync::Arc;

use anyhow::Result;
use protocol::{
    ClientMessage, DisconnectReason, PROTOCOL_VERSION, ResourceAnnouncement,
    ResourceAvailabilityEntry, ResourceAvailabilityReport, ResourceAvailabilityStatus,
    ServerMessage, decode_server_line, encode_line,
};
use server::{
    CapabilityPolicy, EnforcementSection, ProtocolSection, ServerConfig, ServerSection,
    SessionEnforcementPolicy, SharedState, run_with_listener_and_state,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};

fn make_config(addr: &str, strict: bool) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "enforcement test".to_string(),
            ..ServerSection::default()
        },
        enforcement: EnforcementSection {
            mode: if strict {
                SessionEnforcementPolicy::Strict
            } else {
                SessionEnforcementPolicy::ReportOnly
            },
        },
        ..ServerConfig::default()
    }
}

fn make_capability_config(addr: &str, capability_policy: CapabilityPolicy) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "capability enforcement test".to_string(),
            ..ServerSection::default()
        },
        protocol: ProtocolSection {
            capability_policy,
            ..ProtocolSection::default()
        },
        ..ServerConfig::default()
    }
}

fn missing_required_capability_login(name: &str) -> ClientMessage {
    ClientMessage::Login {
        name: name.to_string(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: protocol::LoginCapabilities {
            required: vec![protocol::ProtocolCapability::ResourceAnnouncement],
            optional: vec![protocol::ProtocolCapability::JoinGateDryRun],
            feature_flags: None,
        },
    }
}

fn warning_only_capability_login(name: &str) -> ClientMessage {
    ClientMessage::Login {
        name: name.to_string(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: protocol::LoginCapabilities {
            required: vec![
                protocol::ProtocolCapability::ResourceAnnouncement,
                protocol::ProtocolCapability::ResourceAvailabilityReport,
            ],
            optional: vec![protocol::ProtocolCapability::JoinGateDryRun],
            feature_flags: Some(vec!["unknown_feature".to_string()]),
        },
    }
}

#[tokio::test]
async fn report_only_successful_handshake_reaches_ready_dry_run() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), false);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    let report = build_available_report(&announcement);
    writer_half
        .write_all(encode_line(&ClientMessage::ResourceAvailabilityReport(report))?.as_bytes())
        .await?;

    let mut saw_chat = false;
    loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(_) => break,
            ServerMessage::ChatBroadcast { from, message } => {
                if from == "server" && message == "alice joined" {
                    saw_chat = true;
                }
            }
            ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected message: {other:?}"),
        }
    }
    assert!(saw_chat);

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(snap.ready_dry_run_sessions, 1);
    assert_eq!(snap.failed_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_successful_handshake_reaches_ready_dry_run() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), true);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "bob".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    let report = build_available_report(&announcement);
    writer_half
        .write_all(encode_line(&ClientMessage::ResourceAvailabilityReport(report))?.as_bytes())
        .await?;

    let mut saw_chat = false;
    loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(_) => break,
            ServerMessage::ChatBroadcast { from, message } => {
                if from == "server" && message == "bob joined" {
                    saw_chat = true;
                }
            }
            ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected message: {other:?}"),
        }
    }
    assert!(saw_chat);

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(snap.ready_dry_run_sessions, 1);
    assert_eq!(snap.failed_sessions, 0);
    assert_eq!(snap.sessions[0].event_count, 11);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_version_mismatch_disconnects() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), true);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION + 1,
                capabilities: protocol::current_login_capabilities(),
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
async fn strict_invalid_first_message_disconnects() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), true);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

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
async fn strict_handshake_cleans_up_registry_on_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), true);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        server_state
            .registry
            .lock()
            .unwrap()
            .snapshot()
            .connected_sessions,
        1
    );

    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "alice".to_string(),
                protocol_version: PROTOCOL_VERSION + 1,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _disconnect = read_packet(&mut lines).await?;

    tokio::time::sleep(Duration::from_millis(100)).await;
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0);
    assert_eq!(snap.failed_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn report_only_missing_required_capability_does_not_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_capability_config(&addr.to_string(), CapabilityPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(encode_line(&missing_required_capability_login("cap-report-only"))?.as_bytes())
        .await?;

    let welcome = read_packet(&mut lines).await?;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }));

    let snap = server_state.registry.lock().unwrap().snapshot();
    let report = snap.sessions[0].capability_negotiation.clone().unwrap();
    assert_eq!(report.decision.to_text(), "would_reject");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_missing_required_capability_disconnects() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_capability_config(&addr.to_string(), CapabilityPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(encode_line(&missing_required_capability_login("cap-strict"))?.as_bytes())
        .await?;

    let disconnect = read_packet(&mut lines).await?;
    match disconnect {
        ServerMessage::Disconnect { reason, message } => {
            assert_eq!(reason, DisconnectReason::InvalidHandshake);
            assert!(message.contains("missing required capability"));
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
async fn strict_warning_only_capability_negotiation_still_allows_handshake() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_capability_config(&addr.to_string(), CapabilityPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(encode_line(&warning_only_capability_login("cap-warn-strict"))?.as_bytes())
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    writer_half
        .write_all(
            encode_line(&ClientMessage::ResourceAvailabilityReport(
                build_available_report(&announcement),
            ))?
            .as_bytes(),
        )
        .await?;

    loop {
        match read_packet(&mut lines).await? {
            ServerMessage::JoinGateDecision(_) => break,
            ServerMessage::ChatBroadcast { .. } | ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected message: {other:?}"),
        }
    }

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(snap.ready_dry_run_sessions, 1);
    let report = snap.sessions[0].capability_negotiation.clone().unwrap();
    assert_eq!(report.decision.to_text(), "accepted_with_warnings");

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
