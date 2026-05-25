use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::journal::{JournalReader, LockScope};
use crate::snapshot::ManifestSnapshot;
use crate::state::{canonical_hash, new_id, now_unix_ms};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockLease {
    pub schema_version: u32,
    pub lock_id: String,
    pub owner_id: String,
    pub acquired_at_unix_ms: u64,
    pub lease_duration_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lock_scope: LockScope,
    /// sha256(canonical(lock_scope)) — lease validity tied to scope, not just owner+time.
    pub scope_hash: String,
    pub precondition_hash: String,
    pub last_journal_sequence_at_acquisition: u64,
    pub writer_id: String,
}

#[derive(Debug, Clone)]
pub struct LeaseConfig {
    pub owner_id: String,
    pub writer_id: String,
    pub lease_duration_ms: u64,
}

#[derive(Debug)]
pub enum LeaseReclaimOutcome {
    Reclaimed,
    /// One or more reclamation conditions failed.
    RequiresRevalidation {
        reason: String,
    },
}

impl LockLease {
    pub fn is_expired(&self) -> bool {
        now_unix_ms() >= self.expires_at_unix_ms
    }
}

pub async fn acquire_lease(
    scope: &LockScope,
    last_journal_sequence: u64,
    precondition_hash: String,
    config: &LeaseConfig,
    lock_dir: &std::path::Path,
) -> Result<LockLease> {
    let acquired_at = now_unix_ms();
    let scope_hash = canonical_hash(scope)?;
    let lease = LockLease {
        schema_version: 1,
        lock_id: new_id(),
        owner_id: config.owner_id.clone(),
        acquired_at_unix_ms: acquired_at,
        lease_duration_ms: config.lease_duration_ms,
        expires_at_unix_ms: acquired_at + config.lease_duration_ms,
        lock_scope: scope.clone(),
        scope_hash,
        precondition_hash,
        last_journal_sequence_at_acquisition: last_journal_sequence,
        writer_id: config.writer_id.clone(),
    };
    let path = lock_dir.join(format!("lock_{}.json", lease.lock_id));
    let json = serde_json::to_string_pretty(&lease)?;
    tokio::fs::write(&path, json)
        .await
        .with_context(|| format!("failed to write lock file: {}", path.display()))?;
    Ok(lease)
}

pub async fn release_lease(lease: &LockLease, lock_dir: &std::path::Path) -> Result<()> {
    let path = lock_dir.join(format!("lock_{}.json", lease.lock_id));
    if path.exists() {
        tokio::fs::remove_file(&path)
            .await
            .with_context(|| format!("failed to release lock file: {}", path.display()))?;
    }
    Ok(())
}

/// Attempt to reclaim a stale lease.
///
/// All three conditions must hold:
/// 1. Lease is expired (time).
/// 2. Current manifest precondition_hash matches lease.precondition_hash (state).
/// 3. No journal entries after lease.last_journal_sequence_at_acquisition have a
///    lock_scope overlapping lease.lock_scope (journal consistency).
pub async fn try_reclaim_lease(
    lease: &LockLease,
    current_manifest: &ManifestSnapshot,
    journal: &JournalReader,
) -> Result<LeaseReclaimOutcome> {
    if !lease.is_expired() {
        return Ok(LeaseReclaimOutcome::RequiresRevalidation {
            reason: "lease has not yet expired".to_string(),
        });
    }

    let current_hash = canonical_hash(&current_manifest.entries)?;
    if current_hash != lease.precondition_hash {
        return Ok(LeaseReclaimOutcome::RequiresRevalidation {
            reason: format!(
                "manifest changed since acquisition: expected={} current={}",
                lease.precondition_hash, current_hash
            ),
        });
    }

    let entries_since = journal
        .read_after(lease.last_journal_sequence_at_acquisition)
        .await?;

    let conflict = entries_since.iter().any(|entry| {
        let Some(entry_scope) = &entry.lock_scope else {
            return false;
        };
        scopes_overlap(entry_scope, &lease.lock_scope)
    });

    if conflict {
        return Ok(LeaseReclaimOutcome::RequiresRevalidation {
            reason: "journal entries after acquisition overlap with lock scope".to_string(),
        });
    }

    Ok(LeaseReclaimOutcome::Reclaimed)
}

fn scopes_overlap(a: &LockScope, b: &LockScope) -> bool {
    if a.resource_name != b.resource_name {
        return false;
    }
    match (&a.file_path, &b.file_path) {
        (None, _) | (_, None) => true, // either is resource-wide
        (Some(fa), Some(fb)) => fa == fb,
    }
}
