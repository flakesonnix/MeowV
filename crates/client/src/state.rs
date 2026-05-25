use anyhow::Result;
use serde::Serialize;

use crate::fetch::CacheManifestEntry;
use crate::journal::JournalEntry;
use crate::snapshot::ManifestSnapshot;

/// Committed state: verified snapshot paired with the journal entry that produced it.
#[derive(Debug, Clone)]
pub struct CommittedState {
    pub snapshot: ManifestSnapshot,
    pub journal_head_hash: String,
}

/// In-memory derived view during replay. Never persisted. Dropped on any failure.
#[derive(Debug, Clone)]
pub struct StateView {
    pub entries: Vec<CacheManifestEntry>,
    pub current_hash: String,
}

impl StateView {
    pub fn from_snapshot(snapshot: &ManifestSnapshot) -> Self {
        let entries = snapshot.entries.clone();
        let hash = canonical_hash(&entries).unwrap_or_default();
        Self {
            entries,
            current_hash: hash,
        }
    }

    pub fn apply_entry(&mut self, _entry: &JournalEntry) -> Result<(), ReplayViewError> {
        todo!("M6.16: apply mutation from journal entry to in-memory state")
    }

    pub fn into_snapshot(
        self,
        journal_head_hash: String,
        parent: &ManifestSnapshot,
    ) -> ManifestSnapshot {
        let snapshot_hash = canonical_hash(&self.entries).unwrap_or_default();
        ManifestSnapshot {
            schema_version: 1,
            snapshot_id: new_id(),
            timestamp_unix_ms: now_unix_ms(),
            journal_head_hash,
            parent_snapshot_hash: parent.snapshot_hash.clone(),
            snapshot_hash,
            entries: self.entries,
        }
    }
}

#[derive(Debug)]
pub enum ReplayViewError {
    PreconditionMismatch {
        entry_id: String,
        expected: String,
        actual: String,
    },
    PostconditionMismatch {
        entry_id: String,
        expected: String,
        actual: String,
    },
    UnsupportedMutationType(String),
}

impl std::fmt::Display for ReplayViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreconditionMismatch { entry_id, expected, actual } =>
                write!(f, "precondition mismatch: entry={entry_id} expected={expected} actual={actual}"),
            Self::PostconditionMismatch { entry_id, expected, actual } =>
                write!(f, "postcondition mismatch: entry={entry_id} expected={expected} actual={actual}"),
            Self::UnsupportedMutationType(t) =>
                write!(f, "unsupported mutation type: {t}"),
        }
    }
}

impl std::error::Error for ReplayViewError {}

/// Canonical JSON hash — single implementation used everywhere.
/// Serializes to JSON then SHA-256 hashes; returns `"sha256:<hex>"`.
pub fn canonical_hash(value: &impl Serialize) -> Result<String> {
    use sha2::Digest;
    let json = serde_json::to_string(value)?;
    let digest = sha2::Sha256::digest(json.as_bytes());
    Ok(format!("sha256:{:x}", digest))
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
