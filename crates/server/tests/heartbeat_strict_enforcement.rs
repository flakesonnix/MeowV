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
            motd: "strict heartbeat enforcement test".to_string(),
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
            name: "strict_hb_test".to_string(),
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
async fn report_only_never_disconnects_for_missed_server_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // Fast interval, ReportOnly — threshold can be exceeded, session must stay.
    let config = make_config(&addr.to_string(), 20, HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as u64;
    // Wait well beyond the point where threshold pings would fire.
    sleep(Duration::from_millis((threshold + 3) * 20 + 100)).await;

    // Drain any pending messages without blocking.
    while let Ok(Ok(Some(_))) = timeout(Duration::from_millis(5), lines.next_line()).await {}

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(
        snap.connected_sessions, 1,
        "ReportOnly must never disconnect on missed pong"
    );
    assert_eq!(snap.sessions[0].server_pong_received_count, 0);
    assert!(snap.sessions[0].server_ping_sent_count >= threshold as usize);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_healthy_heartbeat_keeps_session_connected() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // Strict policy — client replies promptly to every ping, must stay connected.
    let config = make_config(&addr.to_string(), 40, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Reply to more than the threshold number of pings.
    let reply_count = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as usize + 2;
    for _ in 0..reply_count {
        let seq = timeout(
            Duration::from_millis(500),
            read_until_server_ping(&mut lines),
        )
        .await??;
        w.write_all(encode_line(&ClientMessage::ServerPong { sequence: seq })?.as_bytes())
            .await?;
    }

    sleep(Duration::from_millis(30)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(
        snap.connected_sessions, 1,
        "healthy Strict client must stay connected"
    );
    let entry = &snap.sessions[0];
    assert_eq!(entry.server_pong_received_count, reply_count);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_missed_server_pong_threshold_disconnects_session() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    // Strict policy, fast interval, client sends no pong replies.
    let config = make_config(&addr.to_string(), 15, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, mut lines) = connect_and_complete_handshake(addr).await?;

    // Wait for server to detect threshold missed pongs and close the connection.
    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as u64;
    let wait_budget_ms = (threshold + 3) * 15 + 300;

    let stream_closed = timeout(Duration::from_millis(wait_budget_ms), async {
        loop {
            match lines.next_line().await {
                Ok(None) => return true, // EOF — server closed the connection
                Ok(Some(_)) => {}        // keep draining (may include Disconnect message)
                Err(_) => return true,   // I/O error also means connection closed
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        stream_closed,
        "expected Strict server to close connection after {} missed pongs",
        threshold
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_registry_cleaned_up_after_heartbeat_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    // Strict policy, fast interval, no pong replies.
    let config = make_config(&addr.to_string(), 15, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (_w, _lines) = connect_and_complete_handshake(addr).await?;

    // Wait for enforcement disconnect + handler cleanup (SessionGuard drop).
    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as u64;
    sleep(Duration::from_millis((threshold + 3) * 15 + 300)).await;

    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(
        snap.connected_sessions, 0,
        "session must be removed from registry after strict heartbeat disconnect"
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_partial_pong_history_still_disconnects_at_threshold() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    // Strict policy. Client replies to first 2 pings, then stops.
    // missed = pings_sent - 2. WouldDisconnect when pings_sent - 2 >= threshold.
    let config = make_config(&addr.to_string(), 30, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as u64;

    // Reply to the first (threshold - 1) pings so pong history is non-zero.
    for _ in 0..(threshold - 1) {
        let seq = timeout(
            Duration::from_millis(500),
            read_until_server_ping(&mut lines),
        )
        .await??;
        w.write_all(encode_line(&ClientMessage::ServerPong { sequence: seq })?.as_bytes())
            .await?;
    }

    // Stop replying. Server must accumulate threshold missed pongs and disconnect.
    // With pongs_received = threshold-1, disconnect fires when:
    //   pings_sent - (threshold-1) >= threshold → pings_sent >= 2*threshold-1
    // Budget: (threshold + 1) more pings × interval + buffer.
    let wait_budget_ms = (threshold + 2) * 30 + 300;

    let stream_closed = timeout(Duration::from_millis(wait_budget_ms), async {
        loop {
            match lines.next_line().await {
                Ok(None) => return true,
                Ok(Some(_)) => {}
                Err(_) => return true,
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        stream_closed,
        "expected Strict server to close connection after threshold missed pongs (with partial history)"
    );

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_enforcement_independent_of_client_ping_activity() -> Result<()> {
    // Client sends client-initiated Pings (and receives Pong replies) throughout the
    // session but never replies to ServerPong. Strict enforcement must still fire on
    // the server-initiated direction — the two directions are independent.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = make_config(&addr.to_string(), 15, HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let (mut w, mut lines) = connect_and_complete_handshake(addr).await?;

    let threshold = MISSED_SERVER_PONG_DISCONNECT_THRESHOLD as u64;
    let wait_budget_ms = (threshold + 3) * 15 + 300;

    let stream_closed = timeout(Duration::from_millis(wait_budget_ms), async {
        let mut seq = 1u64;
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(None) | Err(_) => return true, // EOF — server closed connection
                        Ok(Some(line_str)) => {
                            match protocol::decode_server_line(&line_str) {
                                Ok(ServerMessage::ServerPing { .. }) => {
                                    // Intentionally ignore — do not reply with ServerPong
                                }
                                Ok(ServerMessage::Pong { .. }) | Ok(ServerMessage::EntitySnapshot { .. }) | Ok(ServerMessage::ChatBroadcast { .. }) => {}
                                Ok(ServerMessage::Disconnect { .. }) => return true,
                                _ => {}
                            }
                        }
                    }
                }
                // Send a client-initiated Ping every 10 ms to exercise the other direction.
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    let _ = w.write_all(
                        protocol::encode_line(&ClientMessage::Ping { sequence: seq })
                            .unwrap()
                            .as_bytes(),
                    ).await;
                    seq += 1;
                }
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        stream_closed,
        "Strict enforcement must disconnect even when client-initiated heartbeat direction is healthy"
    );

    server_task.abort();
    Ok(())
}
