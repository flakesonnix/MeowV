use anyhow::Result;
use serde::Serialize;

use crate::fetch::CacheManifestEntry;
use crate::journal::{EntryUpsertPayload, JournalEntry, ManifestRebuildPayload, MutationType};
use crate::snapshot::{ManifestSnapshot, compute_snapshot_hash};

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
    pub fn from_snapshot(snapshot: &ManifestSnapshot) -> Result<Self> {
        let entries = snapshot.entries.clone();
        let hash = canonical_hash(&entries)?;
        Ok(Self { entries, current_hash: hash })
    }

    pub fn apply_entry(&mut self, entry: &JournalEntry) -> Result<(), ReplayViewError> {
        // Precondition: current entries must hash to what the journal entry expects.
        let actual_pre = canonical_hash(&self.entries).map_err(|_| {
            ReplayViewError::PreconditionMismatch {
                entry_id: entry.entry_id.clone(),
                expected: entry.precondition_hash.clone(),
                actual: "<hash error>".to_string(),
            }
        })?;
        if actual_pre != entry.precondition_hash {
            return Err(ReplayViewError::PreconditionMismatch {
                entry_id: entry.entry_id.clone(),
                expected: entry.precondition_hash.clone(),
                actual: actual_pre,
            });
        }

        match &entry.mutation_type {
            MutationType::ManifestEntryRepair | MutationType::FileRefetch => {
                let payload: EntryUpsertPayload =
                    serde_json::from_value(entry.operation_payload.clone()).map_err(|e| {
                        ReplayViewError::UnsupportedMutationType(format!(
                            "{}: bad payload: {e}",
                            entry.mutation_type.label()
                        ))
                    })?;
                upsert_entry(&mut self.entries, payload.entry);
            }
            MutationType::ManifestRebuild => {
                let payload: ManifestRebuildPayload =
                    serde_json::from_value(entry.operation_payload.clone()).map_err(|e| {
                        ReplayViewError::UnsupportedMutationType(format!(
                            "manifest_rebuild: bad payload: {e}"
                        ))
                    })?;
                let mut entries = payload.entries;
                entries.sort_by(|a, b| {
                    (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path))
                });
                self.entries = entries;
            }
        }

        // Postcondition: entries after mutation must hash to what the journal entry asserts.
        let actual_post = canonical_hash(&self.entries).map_err(|_| {
            ReplayViewError::PostconditionMismatch {
                entry_id: entry.entry_id.clone(),
                expected: entry.postcondition_hash.clone(),
                actual: "<hash error>".to_string(),
            }
        })?;
        if actual_post != entry.postcondition_hash {
            return Err(ReplayViewError::PostconditionMismatch {
                entry_id: entry.entry_id.clone(),
                expected: entry.postcondition_hash.clone(),
                actual: actual_post,
            });
        }

        self.current_hash = actual_post;
        Ok(())
    }

    pub fn into_snapshot(
        self,
        journal_head_hash: String,
        parent: &ManifestSnapshot,
    ) -> Result<ManifestSnapshot> {
        let mut snap = ManifestSnapshot {
            schema_version: 1,
            snapshot_id: new_id(),
            timestamp_unix_ms: now_unix_ms(),
            journal_head_hash,
            parent_snapshot_hash: parent.snapshot_hash.clone(),
            snapshot_hash: String::new(),
            entries: self.entries,
        };
        snap.snapshot_hash = compute_snapshot_hash(&snap)?;
        Ok(snap)
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
pub fn canonical_hash<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    use sha2::Digest;
    let json = serde_json::to_string(value)?;
    let digest = sha2::Sha256::digest(json.as_bytes());
    Ok(format!("sha256:{:x}", digest))
}

/// Upsert a `CacheManifestEntry` into `entries`, maintaining deterministic sort order.
/// Existing entry with same (resource_name, file_path) is replaced; otherwise appended.
fn upsert_entry(entries: &mut Vec<CacheManifestEntry>, entry: CacheManifestEntry) {
    let key = (entry.resource_name.as_str(), entry.file_path.as_str());
    entries.retain(|e| (e.resource_name.as_str(), e.file_path.as_str()) != key);
    entries.push(entry);
    entries.sort_by(|a, b| {
        (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path))
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::CacheManifestEntry;
    use crate::journal::{
        EntryUpsertPayload, JournalEntry, LockScope, ManifestRebuildPayload, MutationScope,
        MutationType,
    };
    use crate::snapshot::ManifestSnapshot;

    fn make_entry(resource: &str, path: &str, sha: &str) -> CacheManifestEntry {
        CacheManifestEntry {
            resource_name: resource.to_string(),
            file_path: path.to_string(),
            sha256: sha.to_string(),
            size_bytes: 100,
            source_scheme: None,
            source_uri: None,
        }
    }

    fn make_genesis_view(entries: Vec<CacheManifestEntry>) -> StateView {
        let hash = canonical_hash(&entries).unwrap();
        StateView { entries, current_hash: hash }
    }

    fn make_journal_entry(
        mutation_type: MutationType,
        pre: &str,
        post: &str,
        payload: serde_json::Value,
    ) -> JournalEntry {
        JournalEntry {
            schema_version: 1,
            entry_id: "test-entry".to_string(),
            sequence: 1,
            timestamp_unix_ms: 0,
            mutation_type,
            target_scope: MutationScope {
                resource_name: "chat".to_string(),
                file_path: None,
            },
            precondition_hash: pre.to_string(),
            postcondition_hash: post.to_string(),
            operation_payload: payload,
            idempotency_key: String::new(),
            entry_hash: String::new(),
            prev_entry_hash: "genesis".to_string(),
            chain_hash: None,
            lock_scope: Some(LockScope {
                resource_name: "chat".to_string(),
                file_path: None,
            }),
            checkpoint_state: None,
            retry_count: 0,
            origin_context: None,
        }
    }

    #[test]
    fn apply_manifest_entry_repair_inserts_new_entry() {
        let mut view = make_genesis_view(vec![]);
        let pre = canonical_hash(&view.entries).unwrap();
        let new_entry = make_entry("chat", "main.lua", "aaaa");
        let mut after = vec![new_entry.clone()];
        after.sort_by(|a, b| (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path)));
        let post = canonical_hash(&after).unwrap();
        let payload = serde_json::to_value(EntryUpsertPayload { entry: new_entry.clone() }).unwrap();
        let je = make_journal_entry(MutationType::ManifestEntryRepair, &pre, &post, payload);

        view.apply_entry(&je).unwrap();

        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].sha256, "aaaa");
        assert_eq!(view.current_hash, post);
    }

    #[test]
    fn apply_file_refetch_replaces_existing_entry() {
        let old = make_entry("chat", "main.lua", "old-sha");
        let mut view = make_genesis_view(vec![old]);
        let pre = canonical_hash(&view.entries).unwrap();
        let new_entry = make_entry("chat", "main.lua", "new-sha");
        let after = vec![new_entry.clone()];
        let post = canonical_hash(&after).unwrap();
        let payload = serde_json::to_value(EntryUpsertPayload { entry: new_entry }).unwrap();
        let je = make_journal_entry(MutationType::FileRefetch, &pre, &post, payload);

        view.apply_entry(&je).unwrap();

        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].sha256, "new-sha");
    }

    #[test]
    fn apply_manifest_rebuild_replaces_all_entries() {
        let old = make_entry("chat", "main.lua", "old-sha");
        let mut view = make_genesis_view(vec![old]);
        let pre = canonical_hash(&view.entries).unwrap();
        let new_entries = vec![
            make_entry("mod", "config.lua", "sha-cfg"),
            make_entry("chat", "main.lua", "sha-new"),
        ];
        let mut sorted = new_entries.clone();
        sorted.sort_by(|a, b| (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path)));
        let post = canonical_hash(&sorted).unwrap();
        let payload = serde_json::to_value(ManifestRebuildPayload { entries: new_entries }).unwrap();
        let je = make_journal_entry(MutationType::ManifestRebuild, &pre, &post, payload);

        view.apply_entry(&je).unwrap();

        assert_eq!(view.entries.len(), 2);
        // Sorted: chat < mod
        assert_eq!(view.entries[0].resource_name, "chat");
        assert_eq!(view.entries[1].resource_name, "mod");
    }

    #[test]
    fn apply_entry_rejects_precondition_mismatch() {
        let mut view = make_genesis_view(vec![make_entry("chat", "main.lua", "sha")]);
        let payload = serde_json::to_value(EntryUpsertPayload {
            entry: make_entry("chat", "main.lua", "new"),
        })
        .unwrap();
        let je = make_journal_entry(
            MutationType::ManifestEntryRepair,
            "sha256:wrong-pre",
            "sha256:irrelevant",
            payload,
        );

        let err = view.apply_entry(&je).unwrap_err();
        assert!(matches!(err, ReplayViewError::PreconditionMismatch { .. }));
    }

    #[test]
    fn apply_entry_rejects_postcondition_mismatch() {
        let mut view = make_genesis_view(vec![]);
        let pre = canonical_hash(&view.entries).unwrap();
        let payload = serde_json::to_value(EntryUpsertPayload {
            entry: make_entry("chat", "main.lua", "sha"),
        })
        .unwrap();
        let je = make_journal_entry(
            MutationType::ManifestEntryRepair,
            &pre,
            "sha256:wrong-post",
            payload,
        );

        let err = view.apply_entry(&je).unwrap_err();
        assert!(matches!(err, ReplayViewError::PostconditionMismatch { .. }));
    }

    #[test]
    fn apply_entry_chained_mutations_track_hash() {
        let mut view = make_genesis_view(vec![]);

        // First mutation: insert chat/main.lua
        let pre1 = canonical_hash(&view.entries).unwrap();
        let e1 = make_entry("chat", "main.lua", "sha-v1");
        let after1 = vec![e1.clone()];
        let post1 = canonical_hash(&after1).unwrap();
        let p1 = serde_json::to_value(EntryUpsertPayload { entry: e1 }).unwrap();
        view.apply_entry(&make_journal_entry(MutationType::ManifestEntryRepair, &pre1, &post1, p1)).unwrap();

        assert_eq!(view.current_hash, post1);

        // Second mutation: replace chat/main.lua with new sha
        let pre2 = view.current_hash.clone(); // must equal post1
        let e2 = make_entry("chat", "main.lua", "sha-v2");
        let after2 = vec![e2.clone()];
        let post2 = canonical_hash(&after2).unwrap();
        let p2 = serde_json::to_value(EntryUpsertPayload { entry: e2 }).unwrap();
        view.apply_entry(&make_journal_entry(MutationType::FileRefetch, &pre2, &post2, p2)).unwrap();

        assert_eq!(view.entries.len(), 1);
        assert_eq!(view.entries[0].sha256, "sha-v2");
        assert_eq!(view.current_hash, post2);
        assert_ne!(view.current_hash, post1);
    }

    #[test]
    fn into_snapshot_produces_verifiable_snapshot() {
        let entries = vec![make_entry("chat", "main.lua", "sha")];
        let view = make_genesis_view(entries.clone());
        let parent = ManifestSnapshot::genesis(vec![]).unwrap();
        let snap = view.into_snapshot("genesis".to_string(), &parent).unwrap();
        crate::snapshot::verify_snapshot(&snap).unwrap();
    }
}
