use anyhow::Result;
use protocol::{
    ClientMessage, PROTOCOL_VERSION, ResourceAnnouncement, ResourceAvailabilityEntry,
    ResourceAvailabilityReport, ResourceAvailabilityStatus, ServerMessage, decode_server_line,
    encode_line,
};
use server::{
    HeartbeatPolicy, HeartbeatSection, MISSED_SERVER_PONG_DISCONNECT_THRESHOLD, ServerConfig,
    ServerSection, SharedState, run_with_listener_and_state,
};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, sleep, timeout},
};

fn make_config(addr: &str, ping_interval_ms: u64, policy: HeartbeatPolicy) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "heartbeat timeout status test".to_string(),
            ..ServerSection::default()
        },
        heartbeat: HeartbeatSection {
            policy,
            server_ping_interval_ms: ping_interval_ms,
        },
        ..ServerConfig::default()
    }
}

async fn read_packet<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<ServerMessage>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = timeout(Duration::from_secs(3), lines.next_line())
        .await??
        .expect("stream closed before packet arrived");
    Ok(decode_server_line(&line)?)
}

async fn read_until_server_ping<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<u64>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let msg = read_packet(lines).await?;
        match msg {
            ServerMessage::ServerPing { sequence } => return Ok(sequence),
            ServerMessage::EntitySnapshot { .. }
            | ServerMessage::ChatBroadcast { .. }
            | ServerMessage::Pong { .. } => {}
            other => panic!("unexpected message while waiting for ServerPing: {other:?}"),
        }
    }
}

async fn connect_and_complete_handshake(
    addr: std::net::SocketAddr,
) -> Result<(
    tokio::net::tcp::OwnedWriteHalf,
    tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
)> {
    let stream = TcpStream::connect(addr).await?;
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();

    w.write_all(
        encode_line(&ClientMessage::Login {
            name: "hb_status_test".to_string(),
            protocol_version: PROTOCOL_VERSION,
            capabilities: protocol::current_login_capabilities(),
        })?
        .as_bytes(),
    )
    .await?;

    let welcome = read_packet(&mut lines).await?;
    assert!(
        matches!(welcome, ServerMessage::Welcome { .. }),
        "expected Welcome, got {welcome:?}"
    );

    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    w.write_all(
        encode_line(&ClientMessage::ResourceAvailabilityReport(build_report(
            &announcement,
        )))?
        .as_bytes(),
    )
    .await?;

    loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::JoinGateDecision(_) => break,
            ServerMessage::ChatBroadcast { .. } | ServerMessage::EntitySnapshot { .. } => {}
            other => panic!("unexpected during handshake drain: {other:?}"),
        }
    }

    Ok((w, lines))
}

fn build_report(announcement: &ResourceAnnouncement) -> ResourceAvailabilityReport {
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn srv_heartbeat_no_activity_when_no_pings_sent() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // interval_ms=0 → scheduler disabled → no ServerPings sent
    let config = make_config(&addr.to_string(), 0, HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, _lines) = connect_and_complete_handshake(addr).await?;
    sleep(Duration::from_millis(60)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.sessions.len(), 1);
    let text = snap.to_diagnostics_text();
    assert!(text.contains("srv_heartbeat=no_activity"), "text: {text}");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn srv_heartbeat_healthy_when_pong_received() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 30, HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    let seq = timeout(
        Duration::from_millis(500),
        read_until_server_ping(&mut lines),
    )
    .await??;

    // Reply immediately — check registry before next ping can fire
    w.write_all(encode_line(&ClientMessage::ServerPong { sequence: seq })?.as_bytes())
        .await?;
    sleep(Duration::from_millis(10)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.sessions[0].server_ping_sent_count, 1);
    assert_eq!(snap.sessions[0].server_pong_received_count, 1);
    let text = snap.to_diagnostics_text();
    assert!(text.contains("srv_heartbeat=healthy"), "text: {text}");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn srv_heartbeat_awaiting_pong_when_no_reply_sent() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // 30ms interval; client never replies
    let config = make_config(&addr.to_string(), 30, HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Wait for first ServerPing
    let _ = timeout(
        Duration::from_millis(500),
        read_until_server_ping(&mut lines),
    )
    .await??;
    // Do not reply — just check after first ping
    sleep(Duration::from_millis(5)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    let entry = &snap.sessions[0];
    // pings sent >= 1, pongs received == 0
    assert!(entry.server_ping_sent_count >= 1);
    assert_eq!(entry.server_pong_received_count, 0);

    // Under ReportOnly with no pong → awaiting_pong (or still awaiting even with multiple pings)
    let text = snap.to_diagnostics_text();
    // missed >= 1, pongs_received == 0 → awaiting_pong (below threshold) OR would_disconnect (>=3, but policy=ReportOnly so never would_disconnect)
    assert!(
        text.contains("srv_heartbeat=awaiting_pong"),
        "expected awaiting_pong, text: {text}"
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn srv_heartbeat_strict_enforcement_removes_session_at_threshold() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // Fast interval, Strict policy, no replies from client.
    // M4.20: server actually disconnects at threshold — session removed from registry.
    let config = make_config(&addr.to_string(), 20, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, _lines) = connect_and_complete_handshake(addr).await?;

    // Wait until >= threshold pings would fire plus handler cleanup time.
    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD;
    let wait_ms = (threshold + 3) * 20 + 150;
    sleep(Duration::from_millis(wait_ms)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(
        snap.connected_sessions, 0,
        "Strict enforcement must remove session after threshold missed server pongs"
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn srv_heartbeat_report_only_never_shows_would_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // Fast interval, ReportOnly policy, no replies from client
    let config = make_config(&addr.to_string(), 20, HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Let >= threshold pings fire
    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD;
    sleep(Duration::from_millis((threshold + 2) * 20 + 50)).await;

    while let Ok(Ok(Some(_))) = timeout(Duration::from_millis(5), lines.next_line()).await {}

    let snap = server_state.registry.lock().unwrap().snapshot();
    let entry = &snap.sessions[0];
    assert!(entry.server_ping_sent_count >= threshold as usize);
    assert_eq!(entry.server_pong_received_count, 0);

    let text = snap.to_diagnostics_text();
    assert!(
        !text.contains("srv_heartbeat=would_disconnect"),
        "ReportOnly must not show would_disconnect, text: {text}"
    );
    assert!(
        text.contains("srv_heartbeat=awaiting_pong"),
        "expected awaiting_pong under ReportOnly, text: {text}"
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn registry_diagnostics_text_shows_srv_heartbeat_label_and_counts() -> Result<()> {
    let mut reg = server::SessionRegistry::new();
    reg.set_heartbeat_policy(HeartbeatPolicy::ReportOnly);
    let id = reg.create_session();
    reg.update_server_heartbeat_counts(&id, 2, 2);
    let text = reg.snapshot().to_diagnostics_text();
    assert!(text.contains("srv_heartbeat=healthy"), "text: {text}");
    assert!(text.contains("srv_ping_tx=2"), "text: {text}");
    assert!(text.contains("srv_pong_rx=2"), "text: {text}");
    Ok(())
}
