use std::sync::Arc;

use anyhow::Result;
use protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION, decode_server_line, encode_line};
use server::{run_with_listener_and_state, ServerConfig, ServerSection, SharedState};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpListener, TcpStream}, time::{timeout, Duration}};

fn server_config(addr: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection { bind_addr: addr.to_string(), tick_rate: 20, motd: "ping test".to_string(), ..ServerSection::default() },
        ..ServerConfig::default()
    }
}

async fn read_packet<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<ServerMessage>
where R: tokio::io::AsyncRead + Unpin,
{
    let line = timeout(Duration::from_secs(2), lines.next_line()).await??.expect("stream closed");
    Ok(decode_server_line(&line)?)
}

#[tokio::test]
async fn handshake_then_ping_pong() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(encode_line(&ClientMessage::Login { name: "bob".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // Welcome
    let _welcome = read_packet(&mut lines).await?;

    // ResourceAnnouncement (ignore)
    let _announcement = read_packet(&mut lines).await?;

    // Send Ping(1)
    writer_half.write_all(encode_line(&ClientMessage::Ping { sequence: 1 })?.as_bytes()).await?;

    // Expect Pong(1) somewhere in the incoming stream; other messages (chat, snapshot) may interleave.
    loop {
        let msg = read_packet(&mut lines).await?;
        match msg {
            ServerMessage::Pong { sequence } => { assert_eq!(sequence, 1); break; }
            ServerMessage::ChatBroadcast { .. } | ServerMessage::EntitySnapshot { .. } | ServerMessage::ResourceAnnouncement(_) => { /* skip */ }
            ServerMessage::Disconnect { .. } => panic!("unexpected disconnect while waiting for pong"),
            other => { /* skip any other messages */ }
        }
    }

    // Send multiple pings and expect matching pongs
    for seq in 2..=4u64 {
        writer_half.write_all(encode_line(&ClientMessage::Ping { sequence: seq })?.as_bytes()).await?;
        loop {
            let msg = read_packet(&mut lines).await?;
            match msg {
                ServerMessage::Pong { sequence } => { assert_eq!(sequence, seq); break; }
                ServerMessage::ChatBroadcast { .. } | ServerMessage::EntitySnapshot { .. } | ServerMessage::ResourceAnnouncement(_) => { /* skip */ }
                ServerMessage::Disconnect { .. } => panic!("unexpected disconnect while waiting for pong"),
                _other => { /* skip any other messages */ }
            }
        }
    }

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn ping_before_login_is_invalid() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = BufReader::new(reader_half).lines();

    // Send Ping before Login
    writer_half.write_all(encode_line(&ClientMessage::Ping { sequence: 123 })?.as_bytes()).await?;

    // Expect Disconnect (invalid handshake)
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line()).await??.expect("stream closed");
    let msg = decode_server_line(&line)?;
    match msg {
        ServerMessage::Disconnect { .. } => {}
        other => panic!("expected Disconnect, got {:?}", other),
    }

    drop(lines);
    drop(writer_half);
    server_task.abort();
    Ok(())
}
