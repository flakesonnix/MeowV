// Library facade for the client crate so integration tests can access helpers.
pub mod heartbeat;

use anyhow::Result;
use tokio::io::{BufReader, Lines};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::Duration;
use protocol::decode_server_line;

/// Perform the minimal client-side handshake steps required before sending a Ping,
/// then send a Ping and wait for the matching Pong with the provided timeout.
/// This composes the lower-level heartbeat helper and is exposed for tests and
/// the CLI manual-ping command.
pub async fn perform_ping_once(
    writer: &mut OwnedWriteHalf,
    reader_lines: &mut Lines<BufReader<OwnedReadHalf>>,
    sequence: u64,
    timeout: Duration,
) -> Result<()> {
    // Consume the expected initial server messages (welcome, announcement) so the
    // subsequent Ping is valid in the session flow. Tolerate missing messages.
    if let Ok(Some(line)) = reader_lines.next_line().await {
        let _ = decode_server_line(&line)?;
    }
    if let Ok(Some(line)) = reader_lines.next_line().await {
        let _ = decode_server_line(&line)?;
    }

    crate::heartbeat::send_ping_and_wait_with_timeout(writer, reader_lines, sequence, timeout).await?;
    Ok(())
}

// Keep binary in src/main.rs unchanged; lib only exposes small helpers for tests.
