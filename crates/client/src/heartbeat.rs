use anyhow::Result;
use tokio::io::Lines;
use tokio::io::BufReader;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::time::{timeout, Duration};

use protocol::{ClientMessage, ServerMessage, encode_line, decode_server_line};

/// Send a Ping with the given sequence and await a matching Pong.
/// This helper performs one request/response exchange and returns when the matching
/// Pong(sequence) is received or times out. It tolerates unrelated interleaved
/// server messages by reading and ignoring them until the matching Pong arrives.
/// Timeout defaults to 2 seconds.
pub async fn send_ping_and_wait(
    writer: &mut OwnedWriteHalf,
    reader: &mut Lines<BufReader<OwnedReadHalf>>,
    sequence: u64,
) -> Result<()> {
    send_ping_and_wait_with_timeout(writer, reader, sequence, Duration::from_secs(2)).await
}

/// Variant that allows a custom timeout duration. Useful for tests.
pub async fn send_ping_and_wait_with_timeout(
    writer: &mut OwnedWriteHalf,
    reader: &mut Lines<BufReader<OwnedReadHalf>>,
    sequence: u64,
    timeout_dur: Duration,
) -> Result<()> {
    // Send Ping
    writer
        .write_all(encode_line(&ClientMessage::Ping { sequence })?.as_bytes())
        .await?;

    // Wait for matching Pong within timeout_dur
    loop {
        let line = timeout(timeout_dur, reader.next_line()).await??.expect("stream closed");
        let packet = decode_server_line(&line)?;
        match packet {
            ServerMessage::Pong { sequence: got } if got == sequence => return Ok(()),
            ServerMessage::ServerPing { sequence: srv_seq } => {
                let pong = encode_line(&ClientMessage::ServerPong { sequence: srv_seq });
                if let Ok(line) = pong {
                    let _ = writer.write_all(line.as_bytes()).await;
                }
            }
            _ => continue,
        }
    }
}
