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
    let handle = tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(50), tokio::time::Duration::from_millis(20), stop_rx).await
    });

    // Let a few heartbeats run
    sleep(tokio::time::Duration::from_millis(200)).await;

    // stop the loop
    let _ = stop_tx.send(());

    let metrics = tokio::time::timeout(tokio::time::Duration::from_millis(300), handle).await??;
    assert!(metrics.sent_count >= 1);
    assert!(metrics.pong_count >= 1);
    assert_eq!(metrics.timeout_or_error_count, 0);
    assert_eq!(metrics.last_ping_sequence, metrics.last_pong_sequence);

    server_task.abort();
    Ok(())
}


#[tokio::test]
async fn heartbeat_loop_stops_when_stop_signal_received() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(protocol::encode_line(&protocol::ClientMessage::Login { name: "hb-stop-test".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = std::sync::Arc::new(tokio::sync::Mutex::new(lines));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let hb_writer = writer.clone();
    let hb_lines = lines_arc.clone();
    let handle = tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(50), tokio::time::Duration::from_millis(20), stop_rx).await
    });

    // let it run briefly
    sleep(tokio::time::Duration::from_millis(120)).await;

    // send stop and ensure task completes within reasonable time
    let _ = stop_tx.send(());
    let res = tokio::time::timeout(tokio::time::Duration::from_millis(300), handle).await;
    let metrics = res.expect("heartbeat task did not stop after stop signal")?;
    assert!(metrics.sent_count >= 1);

    server_task.abort();
    Ok(())
}


#[tokio::test]
async fn heartbeat_shutdown_does_not_require_stdin_eof() -> Result<()> {
    // This test ensures that the heartbeat lifecycle does not rely on stdin EOF.
    // It is similar to the stop-signal test but asserts the stop channel is sufficient
    // to terminate the loop without interacting with stdin.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(SharedState::default());
    let config = server_config(&addr.to_string());

    let server_task = tokio::spawn(run_with_listener_and_state(listener, config, state));

    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (reader_half, mut writer_half) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader_half).lines();

    // Login
    writer_half.write_all(protocol::encode_line(&protocol::ClientMessage::Login { name: "hb-no-stdin".to_string(), protocol_version: PROTOCOL_VERSION })?.as_bytes()).await?;

    // consume welcome and announcement
    let _ = lines.next_line().await?;
    let _ = lines.next_line().await?;

    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer_half));
    let lines_arc = std::sync::Arc::new(tokio::sync::Mutex::new(lines));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();

    let hb_writer = writer.clone();
    let hb_lines = lines_arc.clone();
    let handle = tokio::spawn(async move {
        client::heartbeat_loop(hb_writer, hb_lines, tokio::time::Duration::from_millis(50), tokio::time::Duration::from_millis(20), stop_rx).await
    });

    // stop immediately without touching stdin
    let _ = stop_tx.send(());
    let res = tokio::time::timeout(tokio::time::Duration::from_millis(300), handle).await;
    let metrics = res.expect("heartbeat task did not stop after stop signal (no stdin)")?;
    assert_eq!(metrics.sent_count, 0);
    assert_eq!(metrics.pong_count, 0);
    assert_eq!(metrics.timeout_or_error_count, 0);

    server_task.abort();
    Ok(())
}
