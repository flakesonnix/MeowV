use anyhow::{Context, Result};

use crate::journal::{JournalEntry, JournalReader};
use crate::snapshot::{ManifestSnapshot, verify_snapshot, write_snapshot};
use crate::state::StateView;

#[derive(Debug)]
pub enum ReplayError {
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
    HashChainBreak {
        at_sequence: u64,
        expected_prev: String,
        actual_prev: String,
    },
    SnapshotVerificationFailed {
        snapshot_id: String,
        reason: String,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreconditionMismatch {
                entry_id,
                expected,
                actual,
            } => write!(
                f,
                "precondition mismatch: entry={entry_id} expected={expected} actual={actual}"
            ),
            Self::PostconditionMismatch {
                entry_id,
                expected,
                actual,
            } => write!(
                f,
                "postcondition mismatch: entry={entry_id} expected={expected} actual={actual}"
            ),
            Self::HashChainBreak {
                at_sequence,
                expected_prev,
                actual_prev,
            } => write!(
                f,
                "hash chain break at sequence={at_sequence}: expected_prev={expected_prev} actual_prev={actual_prev}"
            ),
            Self::SnapshotVerificationFailed {
                snapshot_id,
                reason,
            } => write!(
                f,
                "snapshot verification failed: id={snapshot_id} reason={reason}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Replay journal entries after the snapshot boundary, producing a new committed snapshot.
///
/// # Invariants enforced
///
/// - Replay reads only from `snapshot` and `journal`. No disk state is consulted.
/// - No partial state is persisted. On any error, in-memory state is discarded.
/// - Replay is full and deterministic from the snapshot baseline.
/// - No optimization shortcut ("reuse existing entries that match") is permitted.
pub async fn replay_journal(
    snapshot: &ManifestSnapshot,
    journal: &JournalReader,
    snapshot_dir: &std::path::Path,
) -> Result<ManifestSnapshot> {
    verify_snapshot(snapshot).with_context(|| {
        format!(
            "refusing to replay from unverified snapshot: {}",
            snapshot.snapshot_id
        )
    })?;

    // Derive sequence from which to read journal entries.
    // We use the journal_head_hash to identify the boundary entry.
    let after_sequence = sequence_from_journal_head(journal, &snapshot.journal_head_hash).await?;
    let entries = journal.read_after(after_sequence).await?;

    if entries.is_empty() {
        return Ok(snapshot.clone());
    }

    // Verify linear hash chain before applying anything.
    verify_hash_chain(snapshot, &entries)?;

    // Apply entries to in-memory state only.
    // FORBIDDEN: do not read disk, do not persist partial state.
    let mut view = StateView::from_snapshot(snapshot)
        .context("failed to initialize state view from snapshot")?;
    for entry in &entries {
        view.apply_entry(entry)
            .map_err(|e| anyhow::anyhow!("replay failed at entry {}: {}", entry.entry_id, e))?;
    }

    let last_entry_hash = entries
        .last()
        .map(|e| e.entry_hash.clone())
        .unwrap_or_else(|| snapshot.journal_head_hash.clone());

    // Only persist after full successful replay.
    let new_snapshot = view
        .into_snapshot(last_entry_hash, snapshot)
        .context("failed to compute snapshot hash after replay")?;
    write_snapshot(snapshot_dir, &new_snapshot).await?;

    Ok(new_snapshot)
}

fn verify_hash_chain(snapshot: &ManifestSnapshot, entries: &[JournalEntry]) -> Result<()> {
    let mut prev_hash: &str = &snapshot.journal_head_hash;
    for entry in entries {
        if entry.prev_entry_hash != *prev_hash {
            return Err(anyhow::anyhow!(ReplayError::HashChainBreak {
                at_sequence: entry.sequence,
                expected_prev: prev_hash.to_owned(),
                actual_prev: entry.prev_entry_hash.clone(),
            }));
        }
        prev_hash = &entry.entry_hash;
    }
    Ok(())
}

/// Locate the sequence number of the entry matching journal_head_hash,
/// so we can read only entries after that boundary.
async fn sequence_from_journal_head(
    journal: &JournalReader,
    journal_head_hash: &str,
) -> Result<u64> {
    if journal_head_hash == "genesis" {
        return Ok(0);
    }
    // Read all entries to find the one with entry_hash == journal_head_hash.
    let all = journal.read_after(0).await?;
    let head = all
        .iter()
        .find(|e| e.entry_hash == journal_head_hash)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "journal_head_hash {} not found in journal; snapshot may be orphaned",
                journal_head_hash
            )
        })?;
    Ok(head.sequence)
}
