use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::fetch::CacheManifestEntry;
use crate::state::{canonical_hash, new_id, now_unix_ms};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSnapshot {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub timestamp_unix_ms: u64,
    /// entry_hash of the last applied journal entry; replay resume point.
    pub journal_head_hash: String,
    /// snapshot_hash of the preceding snapshot; "genesis" for first.
    pub parent_snapshot_hash: String,
    /// sha256 of canonical JSON of this snapshot with this field set to "".
    pub snapshot_hash: String,
    pub entries: Vec<CacheManifestEntry>,
}

impl ManifestSnapshot {
    pub fn genesis(entries: Vec<CacheManifestEntry>) -> Result<Self> {
        let mut snap = Self {
            schema_version: 1,
            snapshot_id: new_id(),
            timestamp_unix_ms: now_unix_ms(),
            journal_head_hash: "genesis".to_string(),
            parent_snapshot_hash: "genesis".to_string(),
            snapshot_hash: String::new(),
            entries,
        };
        snap.snapshot_hash = compute_snapshot_hash(&snap)?;
        Ok(snap)
    }
}

/// Compute snapshot_hash: canonical hash with snapshot_hash field zeroed.
pub(crate) fn compute_snapshot_hash(snap: &ManifestSnapshot) -> Result<String> {
    let mut copy = snap.clone();
    copy.snapshot_hash = String::new();
    canonical_hash(&copy)
}

pub async fn load_latest_snapshot(snapshot_dir: &std::path::Path) -> Result<ManifestSnapshot> {
    let pointer_path = snapshot_dir.join("snapshot_head.json");
    let raw = tokio::fs::read_to_string(&pointer_path)
        .await
        .with_context(|| format!("failed to read snapshot pointer: {}", pointer_path.display()))?;
    let snapshot: ManifestSnapshot =
        serde_json::from_str(&raw).context("failed to parse manifest snapshot")?;
    verify_snapshot(&snapshot)?;
    Ok(snapshot)
}

pub async fn write_snapshot(
    snapshot_dir: &std::path::Path,
    snapshot: &ManifestSnapshot,
) -> Result<()> {
    verify_snapshot(snapshot)?;

    // Write content-addressed file first.
    let content_path = snapshot_dir.join(format!("snapshot_{}.json", snapshot.snapshot_id));
    let json = serde_json::to_string_pretty(snapshot)?;
    tokio::fs::write(&content_path, &json)
        .await
        .with_context(|| format!("failed to write snapshot file: {}", content_path.display()))?;

    // Atomically update the head pointer.
    let tmp_path = snapshot_dir.join("snapshot_head.json.tmp");
    tokio::fs::write(&tmp_path, &json)
        .await
        .context("failed to write snapshot pointer tmp")?;
    tokio::fs::rename(&tmp_path, snapshot_dir.join("snapshot_head.json"))
        .await
        .context("failed to atomically replace snapshot pointer")?;

    Ok(())
}

pub fn verify_snapshot(snapshot: &ManifestSnapshot) -> Result<()> {
    let expected = compute_snapshot_hash(snapshot)?;
    anyhow::ensure!(
        snapshot.snapshot_hash == expected,
        "snapshot_hash mismatch: id={} expected={} actual={}",
        snapshot.snapshot_id,
        expected,
        snapshot.snapshot_hash,
    );
    Ok(())
}
