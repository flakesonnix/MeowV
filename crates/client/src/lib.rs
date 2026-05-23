// Library facade for the client crate so integration tests can access helpers.
pub mod heartbeat;

use anyhow::Result;
use tokio::io::{BufReader, Lines};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::time::Duration;
use protocol::decode_server_line;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

/// Start a periodic heartbeat loop that sends pings at `interval` and waits for
/// matching pongs with `timeout`. The loop runs until `stop_rx` receives a
/// message. This function runs the loop inline; callers usually spawn it as a
/// background task.
pub async fn heartbeat_loop(
    writer: Arc<AsyncMutex<OwnedWriteHalf>>,
    lines: Arc<AsyncMutex<Lines<BufReader<OwnedReadHalf>>>>,
    interval: Duration,
    timeout: Duration,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let mut sequence: u64 = 1;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Acquire locks for writer and reader, then perform ping+wait.
                let mut wguard = writer.lock().await;
                // Lock lines
                let mut lguard = lines.lock().await;
                // Call the helper with mutable refs
                let res = crate::heartbeat::send_ping_and_wait_with_timeout(&mut *wguard, &mut *lguard, sequence, timeout).await;
                match res {
                    Ok(()) => tracing::info!("Heartbeat {}: Pong received", sequence),
                    Err(e) => tracing::warn!("Heartbeat {}: failed: {}", sequence, e),
                }
                sequence = sequence.saturating_add(1);
            }
            _ = &mut stop_rx => {
                tracing::info!("heartbeat loop stopping");
                break;
            }
        }
    }
}

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
