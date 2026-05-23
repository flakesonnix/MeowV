use std::process::Command;
use anyhow::Result;
use tokio::{net::TcpListener, sync::oneshot};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use protocol::{PROTOCOL_VERSION};
use server::{run_with_listener_and_state, ServerConfig, ServerSection, SharedState};
use std::sync::Arc;

fn server_config(addr: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection { bind_addr: addr.to_string(), tick_rate: 20, motd: "cli ping test".to_string(), ..ServerSection::default() },
        ..ServerConfig::default()
    }
}

#[tokio::test]
async fn cli_ping_once_success() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    // Perform the ping flow via the library helper instead of spawning the binary.
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // Send Login
    writer_half.write_all(protocol::encode_line(&protocol::ClientMessage::Login { name: "cli-test".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // Use client library helper
    client::perform_ping_once(&mut writer_half, &mut lines, 1, tokio::time::Duration::from_secs(2)).await?;

    server_task.abort();
    Ok(())
}

#[tokio::test]
async fn cli_ping_once_timeout() -> Result<()> {
    // Spawn a tiny server that accepts but does not reply with Pong
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let (ready_tx, ready_rx) = oneshot::channel::<()>();
    let server_task = tokio::spawn(async move {
        // accept one connection and perform minimal handshake then ignore
        if let Ok((mut socket, _)) = listener.accept().await {
            let (r, mut w) = socket.split();
            let mut reader = tokio::io::BufReader::new(r).lines();
            if let Ok(Some(_line)) = reader.next_line().await {
                // send Welcome and Announcement
                let welcome = protocol::encode_line(&protocol::ServerMessage::Welcome { client_id: uuid::Uuid::new_v4(), motd: "no-pong".to_string(), protocol_version: PROTOCOL_VERSION }).unwrap();
                let announcement = protocol::encode_line(&protocol::ServerMessage::ResourceAnnouncement(protocol::ResourceAnnouncement { resources: vec![], signature: None })).unwrap();
                let _ = w.write_all(welcome.as_bytes()).await;
                let _ = w.write_all(announcement.as_bytes()).await;
            }
            // keep connection open and do not respond to pings
            let _ = ready_tx.send(());
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        }
    });

    // Wait for server to be ready
    let _ = ready_rx.await;

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // Send Login
    writer_half.write_all(protocol::encode_line(&protocol::ClientMessage::Login { name: "cli-test".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // Use client library helper with short timeout
    let res = client::perform_ping_once(&mut writer_half, &mut lines, 99, tokio::time::Duration::from_millis(50)).await;
    assert!(res.is_err(), "expected timeout/failure when server does not respond");

    server_task.abort();
    Ok(())
}
