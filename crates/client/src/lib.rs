// Library facade for the client crate so integration tests can access helpers.
pub mod fetch;
pub mod hash;
pub mod heartbeat;
pub mod journal;
pub mod lock;
pub mod reconciliation;
pub mod repair;
pub mod replay;
pub mod snapshot;
pub mod state;
pub mod trust;

use anyhow::Context;
use anyhow::Result;
use protocol::decode_server_line;
use protocol::{
    ResourceAnnouncement, ResourceAvailabilityEntry, ResourceAvailabilityReport,
    ResourceAvailabilityStatus, check_announcement_signature_stub, evaluate_resource_policy,
};
use resource_manifest::{CacheFileStatus, verify_cache_for_resource};
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{BufReader, Lines};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Duration;

fn read_cli_flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn build_preflight_plan(
    path: &str,
    args: &[String],
    policy: &protocol::signature_engine::SignaturePolicy,
) -> Result<protocol::ResourceDownloadPreflightPlan> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read announcement file: {path}"))?;
    let announcement: ResourceAnnouncement =
        serde_json::from_str(&raw).context("failed to parse ResourceAnnouncement JSON")?;

    let has_trusted_keys = read_cli_flag(args, "--trusted-keys").is_some();
    match policy {
        protocol::signature_engine::SignaturePolicy::Strict if !has_trusted_keys => {
            anyhow::bail!("--signature-policy strict requires --trusted-keys <path>")
        }
        _ => {}
    }

    let resource_cache = read_cli_flag(args, "--resource-cache");
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(".").to_path_buf());

    let mut avail_entries: Vec<ResourceAvailabilityEntry> = Vec::new();
    for resource in &announcement.resources {
        if let Some(cache_dir) = resource_cache.as_deref() {
            let resource_dir = workspace_root.join(format!("examples/resources/{}", resource.name));
            let report = verify_cache_for_resource(&resource_dir, cache_dir)?;
            avail_entries.extend(report.entries.into_iter().map(|entry| {
                ResourceAvailabilityEntry {
                    resource_name: resource.name.clone(),
                    file_path: entry.relative_path.to_string_lossy().into_owned(),
                    status: map_cache_status(entry.status),
                }
            }));
        } else {
            avail_entries.extend(resource.files.iter().map(|file| ResourceAvailabilityEntry {
                resource_name: resource.name.clone(),
                file_path: file.relative_path.clone(),
                status: ResourceAvailabilityStatus::Missing,
            }));
        }
    }

    let is_fully_available = avail_entries
        .iter()
        .all(|entry| entry.status == ResourceAvailabilityStatus::Available);
    let availability_report = ResourceAvailabilityReport {
        resources: avail_entries,
        is_fully_available,
    };

    let signature_report = check_announcement_signature_stub(&announcement);
    let policy_eval = evaluate_resource_policy(&announcement, &availability_report);

    Ok(protocol::build_resource_download_preflight_plan(
        &announcement,
        &availability_report,
        &signature_report,
        policy,
        Some(&policy_eval),
    ))
}

/// Deterministic, report-only resource download preflight planner helper.
/// Mirrors CLI behavior. Does not perform network I/O or cache writes.
pub fn get_resource_download_preflight_plan_text(
    path: &str,
    args: &[String],
    policy: &protocol::signature_engine::SignaturePolicy,
) -> Result<String> {
    Ok(build_preflight_plan(path, args, policy)?.to_text())
}

/// Return the preflight plan as JSON string (deterministic ordering via plan serialization).
pub fn get_resource_download_preflight_plan_json(
    path: &str,
    args: &[String],
    policy: &protocol::signature_engine::SignaturePolicy,
) -> Result<String> {
    Ok(serde_json::to_string_pretty(&build_preflight_plan(
        path, args, policy,
    )?)?)
}

fn map_cache_status(status: CacheFileStatus) -> ResourceAvailabilityStatus {
    match status {
        CacheFileStatus::Valid => ResourceAvailabilityStatus::Available,
        CacheFileStatus::Missing => ResourceAvailabilityStatus::Missing,
        CacheFileStatus::SizeMismatch => ResourceAvailabilityStatus::SizeMismatch,
        CacheFileStatus::HashMismatch => ResourceAvailabilityStatus::HashMismatch,
    }
}

/// Heartbeat enforcement policy for the client-side heartbeat loop.
///
/// `ReportOnly` logs timeouts and errors but never stops the loop due to
/// enforcement. `Strict` disconnects when `timeout_or_error_count` reaches
/// `CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ClientHeartbeatPolicy {
    #[default]
    ReportOnly,
    Strict,
}

/// Threshold at which `Strict` policy triggers an enforcement disconnect.
/// Matches the server-side `MISSED_HEARTBEAT_DISCONNECT_THRESHOLD` constant.
pub const CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD: u64 = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeartbeatMetrics {
    pub sent_count: u64,
    pub pong_count: u64,
    pub timeout_or_error_count: u64,
    pub last_ping_sequence: Option<u64>,
    pub last_pong_sequence: Option<u64>,
    /// Set to `true` when `Strict` policy stopped the loop due to missed heartbeats.
    pub enforcement_disconnect: bool,
}

impl HeartbeatMetrics {
    pub fn to_text(&self) -> String {
        let mut out = format!(
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
        );
        if self.enforcement_disconnect {
            out.push_str("\nheartbeat_enforcement_disconnect: true");
        }
        out
    }
}

impl fmt::Display for HeartbeatMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

fn optional_sequence_text(value: Option<u64>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_string())
}

/// Start a periodic heartbeat loop that sends pings at `interval` and waits for
/// matching pongs with `timeout`. The loop runs until `stop_rx` fires or, under
/// `Strict` policy, until `timeout_or_error_count` reaches
/// `CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD`. Returns accumulated `HeartbeatMetrics`.
pub async fn heartbeat_loop(
    writer: Arc<AsyncMutex<OwnedWriteHalf>>,
    lines: Arc<AsyncMutex<Lines<BufReader<OwnedReadHalf>>>>,
    interval: Duration,
    timeout: Duration,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
    policy: ClientHeartbeatPolicy,
) -> HeartbeatMetrics {
    let mut sequence: u64 = 1;
    let mut metrics = HeartbeatMetrics::default();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                metrics.sent_count = metrics.sent_count.saturating_add(1);
                metrics.last_ping_sequence = Some(sequence);
                let mut wguard = writer.lock().await;
                let mut lguard = lines.lock().await;
                let res = crate::heartbeat::send_ping_and_wait_with_timeout(&mut wguard, &mut lguard, sequence, timeout).await;
                match res {
                    Ok(()) => {
                        metrics.pong_count = metrics.pong_count.saturating_add(1);
                        metrics.last_pong_sequence = Some(sequence);
                        tracing::info!("Heartbeat {}: Pong received", sequence);
                    }
                    Err(e) => {
                        metrics.timeout_or_error_count = metrics.timeout_or_error_count.saturating_add(1);
                        tracing::warn!("Heartbeat {}: failed: {}", sequence, e);
                        if policy == ClientHeartbeatPolicy::Strict
                            && metrics.timeout_or_error_count >= CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD
                        {
                            tracing::warn!(
                                "heartbeat enforcement: strict policy — {} missed heartbeats, disconnecting",
                                metrics.timeout_or_error_count
                            );
                            metrics.enforcement_disconnect = true;
                            break;
                        }
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

    crate::heartbeat::send_ping_and_wait_with_timeout(writer, reader_lines, sequence, timeout)
        .await?;
    Ok(())
}

/// Reply to a server-initiated ServerPing by sending ClientMessage::ServerPong
/// with the same sequence number. This is the client's half of the authoritative
/// liveness path added in M4.16/M4.17.
pub async fn handle_server_ping(writer: &mut OwnedWriteHalf, sequence: u64) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt as _;
    writer
        .write_all(
            protocol::encode_line(&protocol::ClientMessage::ServerPong { sequence })?.as_bytes(),
        )
        .await?;
    Ok(())
}

// Keep binary in src/main.rs unchanged; lib only exposes small helpers for tests.
