use anyhow::Result;
use protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, decode_server_line, encode_line};
use server::{
    HeartbeatPolicy, HeartbeatSection, ServerConfig, ServerSection, SharedState,
    run_with_listener_and_state,
};
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{Duration, sleep, timeout},
};

fn make_config(addr: &str, hb_policy: HeartbeatPolicy) -> ServerConfig {
    ServerConfig {
        server: ServerSection {
            bind_addr: addr.to_string(),
            tick_rate: 20,
            motd: "heartbeat enforcement test".to_string(),
            ..ServerSection::default()
        },
        heartbeat: HeartbeatSection {
            policy: hb_policy,
            server_ping_interval_ms: 0,
        },
        ..ServerConfig::default()
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

#[tokio::test]
async fn registry_cleans_up_after_enforced_heartbeat_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "enforcer".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    // Consume Welcome and ResourceAnnouncement
    let _welcome = read_packet(&mut lines).await?;
    let _announcement = read_packet(&mut lines).await?;

    // Verify session appears in registry
    sleep(Duration::from_millis(50)).await;
    assert_eq!(
        server_state
            .registry
            .lock()
            .unwrap()
            .snapshot()
            .connected_sessions,
        1
    );

    // Drop client connection — simulates enforcement disconnect
    drop(writer_half);
    drop(lines);

    // Registry must clean up via SessionGuard on handler exit
    sleep(Duration::from_millis(150)).await;
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 0);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn report_only_heartbeat_policy_session_stays_connected() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), HeartbeatPolicy::ReportOnly);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "observer".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    let _announcement = read_packet(&mut lines).await?;

    // Send a ping and expect a pong (server handles heartbeat)
    writer_half
        .write_all(encode_line(&ClientMessage::Ping { sequence: 1 })?.as_bytes())
        .await?;

    // Absorb messages until we see a Pong (or timeout)
    let got_pong = timeout(Duration::from_millis(500), async {
        loop {
            if let Ok(Some(line)) = lines.next_line().await {
                if let Ok(ServerMessage::Pong { sequence: 1 }) = decode_server_line(&line) {
                    return true;
                }
            } else {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false);

    assert!(
        got_pong,
        "expected Pong from server under ReportOnly policy"
    );

    // Session still connected
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(snap.sessions[0].ping_received_count, 1);

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn strict_heartbeat_policy_session_stays_connected_when_healthy() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let server_state = state.clone();
    let config = make_config(&addr.to_string(), HeartbeatPolicy::Strict);

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    writer_half
        .write_all(
            encode_line(&ClientMessage::Login {
                name: "strict-hb".to_string(),
                protocol_version: PROTOCOL_VERSION,
                capabilities: protocol::current_login_capabilities(),
            })?
            .as_bytes(),
        )
        .await?;

    let _welcome = read_packet(&mut lines).await?;
    let _announcement = read_packet(&mut lines).await?;

    // Send 3 pings and wait for matching pongs — all healthy
    for seq in 1u64..=3 {
        writer_half
            .write_all(encode_line(&ClientMessage::Ping { sequence: seq })?.as_bytes())
            .await?;

        let got_pong = timeout(Duration::from_millis(500), async {
            loop {
                if let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(ServerMessage::Pong { sequence: s }) = decode_server_line(&line) {
                        if s == seq {
                            return true;
                        }
                    }
                } else {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false);

        assert!(got_pong, "expected Pong for sequence {seq}");
    }

    // Session stays connected under Strict when heartbeats are healthy
    let snap = server_state.registry.lock().unwrap().snapshot();
    assert_eq!(snap.connected_sessions, 1);
    assert_eq!(snap.sessions[0].ping_received_count, 3);

    server_task.abort();
    Ok(())
}
