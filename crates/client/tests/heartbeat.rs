use anyhow::Result;
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, net::{TcpListener, TcpStream}, time::Duration};
use protocol::{ClientMessage, ServerMessage, PROTOCOL_VERSION, decode_server_line, encode_line};
use server::{run_with_listener_and_state, ServerConfig, ServerSection, SharedState};
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
