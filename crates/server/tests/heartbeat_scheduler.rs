use anyhow::Result;
use protocol::{
    ClientMessage, PROTOCOL_VERSION, ResourceAnnouncement, ResourceAvailabilityEntry,
    ResourceAvailabilityReport, ResourceAvailabilityStatus, ServerMessage, decode_server_line,
    encode_line,
};
use server::{HeartbeatSection, HeartbeatPolicy, ServerConfig, ServerSection, SharedState, run_with_listener_and_state};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, sleep, timeout},
};

fn make_config(addr: &str, ping_interval_ms: u64) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "scheduler test".to_string(),
            ..ServerSection::default()
        },
        heartbeat: HeartbeatSection {
            policy: HeartbeatPolicy::ReportOnly,
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

/// Read packets until ServerPing is found, skipping EntitySnapshot/ChatBroadcast/Pong.
async fn read_until_server_ping<R>(
    lines: &mut tokio::io::Lines<BufReader<R>>,
) -> Result<u64>
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

/// Connect, complete the full handshake, and return (writer, lines).
/// Consumes Welcome, ResourceAnnouncement, and sends AvailabilityReport.
/// Drains messages until JoinGateDecision.
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
            name: "scheduler_test".to_string(),
            protocol_version: PROTOCOL_VERSION,
        })?
        .as_bytes(),
    )
    .await?;

    let welcome = read_packet(&mut lines).await?;
    assert!(matches!(welcome, ServerMessage::Welcome { .. }), "expected Welcome, got {welcome:?}");

    let announcement = read_packet(&mut lines).await?;
    let announcement = match announcement {
        ServerMessage::ResourceAnnouncement(a) => a,
        other => panic!("expected ResourceAnnouncement, got {other:?}"),
    };

    w.write_all(
        encode_line(&ClientMessage::ResourceAvailabilityReport(build_report(&announcement)))?
            .as_bytes(),
    )
    .await?;

    // Drain until JoinGateDecision
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
    ResourceAvailabilityReport { resources, is_fully_available: true }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn server_sends_server_ping_after_handshake() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = make_config(&addr.to_string(), 30); // 30ms interval

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    let seq = timeout(
        Duration::from_millis(500),
        read_until_server_ping(&mut lines),
    )
    .await??;

    assert_eq!(seq, 1, "first ServerPing must have sequence 1");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn server_ping_sequences_increment() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = make_config(&addr.to_string(), 30);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    let seq1 = timeout(Duration::from_millis(500), read_until_server_ping(&mut lines)).await??;
    let seq2 = timeout(Duration::from_millis(500), read_until_server_ping(&mut lines)).await??;

    assert_eq!(seq1, 1);
    assert_eq!(seq2, 2);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn server_pong_reply_updates_registry_counts() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 30);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Wait for the first ServerPing
    let seq = timeout(Duration::from_millis(500), read_until_server_ping(&mut lines)).await??;
    assert_eq!(seq, 1);

    // Reply with matching ServerPong
    w.write_all(encode_line(&ClientMessage::ServerPong { sequence: 1 })?.as_bytes())
        .await?;

    // Give server time to record the reply
    sleep(Duration::from_millis(50)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.sessions.len(), 1);
    let entry = &snap.sessions[0];
    // Ping count may be > 1 if interval fired multiple times; at least 1 required.
    assert!(entry.server_ping_sent_count >= 1, "ping sent count must be >= 1");
    assert_eq!(entry.server_pong_received_count, 1, "pong received count");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn mismatched_server_pong_sequence_is_not_fatal() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 30);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Wait for first ServerPing
    let _ = timeout(Duration::from_millis(500), read_until_server_ping(&mut lines)).await??;

    // Send ServerPong with wrong sequence (999 instead of 1)
    w.write_all(encode_line(&ClientMessage::ServerPong { sequence: 999 })?.as_bytes())
        .await?;

    sleep(Duration::from_millis(50)).await;

    // Session must still be connected — mismatch is not fatal
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1, "session must still be connected");
    // Pong was still recorded (server doesn't validate sequence in report-only mode)
    assert_eq!(snap.sessions[0].server_pong_received_count, 1);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn report_only_never_disconnects_on_missing_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 20); // fast interval, no replies sent

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Let several ServerPings accumulate without any ServerPong replies
    sleep(Duration::from_millis(200)).await;

    // Drain any pending messages (ServerPing, EntitySnapshot, etc.)
    while let Ok(Ok(Some(line))) = timeout(Duration::from_millis(20), lines.next_line()).await {
        let _ = decode_server_line(&line);
    }

    // Session still connected — ReportOnly never disconnects on missed pongs
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1, "session must still be connected");
    assert_eq!(snap.sessions[0].server_pong_received_count, 0, "no pongs sent");
    assert!(snap.sessions[0].server_ping_sent_count >= 2, "server sent multiple pings");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn session_cleanup_stops_scheduler_state() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 30);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (w, lines) = connect_and_complete_handshake(addr).await?;

    // Let scheduler fire at least once
    sleep(Duration::from_millis(80)).await;
    assert_eq!(server_state.registry.lock().unwrap().snapshot().connected_sessions, 1);

    // Drop the connection
    drop(w);
    drop(lines);

    sleep(Duration::from_millis(100)).await;

    // Session removed — scheduler stopped naturally when handle_client exited
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0, "session must be cleaned up");

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn diagnostics_text_shows_server_heartbeat_counts() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), 30);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    let _ = timeout(Duration::from_millis(500), read_until_server_ping(&mut lines)).await??;
    w.write_all(encode_line(&ClientMessage::ServerPong { sequence: 1 })?.as_bytes())
        .await?;
    sleep(Duration::from_millis(50)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    let diag = snap.to_diagnostics_text();
    // srv_pong_rx=1 is exact (we sent exactly one ServerPong)
    assert!(diag.contains("srv_pong_rx=1"), "diagnostics: {diag}");
    // srv_ping_tx= field must appear; count may be > 1 due to interval timing
    assert!(diag.contains("srv_ping_tx="), "diagnostics: {diag}");

    server_task.abort();
    Ok(())
}
