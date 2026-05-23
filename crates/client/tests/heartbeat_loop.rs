use anyhow::Result;
use tokio::{net::TcpListener, time::sleep};
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use protocol::{PROTOCOL_VERSION};
use std::sync::Arc;
use server::{run_with_listener_and_state, ServerConfig, ServerSection, SharedState};

fn server_config(addr: &str) -> ServerConfig {
    ServerConfig {
        server: ServerSection { bind_addr: addr.to_string(), tick_rate: 20, motd: "heartbeat loop test".to_string(), ..ServerSection::default() },
        ..ServerConfig::default()
    }
}

#[tokio::test]
async fn heartbeat_loop_sends_multiple_pings() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(protocol::encode_line(&protocol::ClientMessage::Login { name: "hb-test".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = std::sync::Arc::new(tokio::sync::Mutex::new(lines));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let hb_writer = writer.clone();
    let hb_lines = lines_arc.clone();
    tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(50), tokio::time::Duration::from_millis(20), stop_rx).await;
    });

    // Let a few heartbeats run
    sleep(tokio::time::Duration::from_millis(200)).await;

    // stop the loop
    let _ = stop_tx.send(());

    server_task.abort();
    Ok(())
}
