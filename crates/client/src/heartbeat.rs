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
    mut writer: &mut OwnedWriteHalf,
    reader: &mut Lines<BufReader<OwnedReadHalf>>,
    sequence: u64,
) -> Result<()> {
    // Send Ping
    writer
        .write_all(encode_line(&ClientMessage::Ping { sequence })?.as_bytes())
        .await?;

    // Wait for matching Pong within 2s
    let deadline = Duration::from_secs(2);
    loop {
        let line = timeout(deadline, reader.next_line()).await??.expect("stream closed");
        let packet = decode_server_line(&line)?;
        match packet {
            ServerMessage::Pong { sequence: got } if got == sequence => return Ok(()),
            // Skip other messages
            _ => continue,
        }
    }
}
