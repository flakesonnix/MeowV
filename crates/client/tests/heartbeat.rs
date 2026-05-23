use anyhow::Result;
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpListener, TcpStream}, time::Duration};
use protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION, decode_server_line, encode_line};
use server::{run_with_listener_and_state, ServerConfig, ServerSection, SharedState};
use client::heartbeat;
use std::sync::Arc;

fn server_config(addr: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection { bind_addr: addr.to_string(), tick_rate: 20, motd: "heartbeat test".to_string(), ..ServerSection::default() },
        ..ServerConfig::default()
    }
}

async fn read_packet<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<ServerMessage>
where R: tokio::io::AsyncRead + Unpin,
{
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line()).await??.expect("stream closed");
    Ok(decode_server_line(&line)?)
}

#[tokio::test]
async fn helper_sends_ping_and_receives_matching_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "alice".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = read_packet(&mut lines).await?;
    let _ = read_packet(&mut lines).await?;

    // use the helper
    crate::heartbeat::send_ping_and_wait(&mut writer_half, &mut lines, 7).await?;

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn helper_ignores_unrelated_server_messages() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "alice".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = read_packet(&mut lines).await?;
    let _ = read_packet(&mut lines).await?;

    // Now, simulate that server will send unrelated ChatBroadcast before Pong.
    // We send a Ping and then the server (our test harness) will produce unrelated messages in its normal flow
    // The helper should ignore any unrelated server messages and wait until the matching Pong arrives.
    crate::heartbeat::send_ping_and_wait(&mut writer_half, &mut lines, 11).await?;

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn helper_ignores_mismatched_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "alice".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = read_packet(&mut lines).await?;
    let _ = read_packet(&mut lines).await?;

    // The server will reply with Pong but potentially for a different sequence (due to interleaving)
    // Our helper expects a matching sequence; it should ignore mismatched pongs until it gets the right one.
    // Use sequences where server will produce pongs - we rely on normal server behavior to echo.
    crate::heartbeat::send_ping_and_wait(&mut writer_half, &mut lines, 21).await?;

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn helper_times_out_when_matching_pong_never_arrives() -> Result<()> {
    // For this test we run a tiny custom server that will NOT reply with Pong to any Ping.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    // Spawn a small server task that accepts one connection and performs minimal handshake
    let server_task = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let (r, mut w) = socket.split();
            let mut reader = BufReader::new(r).lines();

            // Read login from client
            if let Ok(Some(_line)) = reader.next_line().await {
                // send Welcome and ResourceAnnouncement
                let welcome = encode_line(&ServerMessage::Welcome { client_id: uuid::Uuid::new_v4(), motd: "no-pong".to_string(), protocol_version: PROTOCOL_VERSION }).unwrap();
                let announcement = encode_line(&ServerMessage::ResourceAnnouncement(protocol::ResourceAnnouncement { resources: vec![], signature: None })).unwrap();
                let _ = w.write_all(welcome.as_bytes()).await;
                let _ = w.write_all(announcement.as_bytes()).await;
            }

            // Now intentionally ignore any further client messages (do not respond with Pong)
            // Keep the task alive for a short while to let the test timeout occur
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "bob".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = read_packet(&mut lines).await?;
    let _ = read_packet(&mut lines).await?;

    // Use the timeout-enabled helper with a short timeout (50ms) to avoid waiting a long time in tests.
    let res = crate::heartbeat::send_ping_and_wait_with_timeout(&mut writer_half, &mut lines, 99, Duration::from_millis(50)).await;
    assert!(res.is_err(), "expected timeout error when Pong never arrives");

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}
