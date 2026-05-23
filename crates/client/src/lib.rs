// Library facade for the client crate so integration tests can access helpers.
pub mod heartbeat;

use anyhow::Result;
use protocol::decode_server_line;
use std::fmt;
use std::sync::Arc;
use tokio::io::{BufReader, Lines};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeartbeatMetrics {
    pub sent_count: u64,
    pub pong_count: u64,
    pub timeout_or_error_count: u64,
    pub last_ping_sequence: Option<u64>,
    pub last_pong_sequence: Option<u64>,
}

impl HeartbeatMetrics {
    pub fn to_text(&self) -> String {
        format!(
            "heartbeat_sent_count: {}\n\
             heartbeat_pong_count: {}\n\
             heartbeat_timeout_or_error_count: {}\n\
             last_ping_sequence: {}\n\
             last_pong_sequence: {}",
            self.sent_count,
            self.pong_count,
            self.timeout_or_error_count,
            optional_sequence_text(self.last_ping_sequence),
            optional_sequence_text(self.last_pong_sequence),
        )
    }
}

impl fmt::Display for HeartbeatMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

fn optional_sequence_text(value: Option<u64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string())
}

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
) -> HeartbeatMetrics {
    let mut sequence: u64 = 1;
    let mut metrics = HeartbeatMetrics::default();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                metrics.sent_count = metrics.sent_count.saturating_add(1);
                metrics.last_ping_sequence = Some(sequence);
                // Acquire locks for writer and reader, then perform ping+wait.
                let mut wguard = writer.lock().await;
                // Lock lines
                let mut lguard = lines.lock().await;
                // Call the helper with mutable refs
                let res = crate::heartbeat::send_ping_and_wait_with_timeout(&mut *wguard, &mut *lguard, sequence, timeout).await;
                match res {
                    Ok(()) => {
                        metrics.pong_count = metrics.pong_count.saturating_add(1);
                        metrics.last_pong_sequence = Some(sequence);
                        tracing::info!("Heartbeat {}: Pong received", sequence)
                    }
                    Err(e) => {
                        metrics.timeout_or_error_count = metrics.timeout_or_error_count.saturating_add(1);
                        tracing::warn!("Heartbeat {}: failed: {}", sequence, e)
                    }
                }
                sequence = sequence.saturating_add(1);
            }
            _ = &mut stop_rx => {
                tracing::info!("heartbeat loop stopping");
                break;
            }
        }
    }
    metrics
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
