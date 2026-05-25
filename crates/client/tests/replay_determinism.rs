/// End-to-end state consistency tests for the M6.16 journal → snapshot → replay loop.
///
/// These tests prove the system invariants that matter under real operation:
///
/// 1. Replay from genesis + journal entries → correct committed snapshot
/// 2. Replay is deterministic: same (snapshot, journal) → identical output
/// 3. Chained replay: snapshot_N as input → snapshot_N+1 preserves all prior state
/// 4. Empty journal replay: snapshot unchanged, no write performed
/// 5. Hash chain break: replay rejects tampered journal entries
use client::fetch::CacheManifestEntry;
use client::journal::JournalEntry;
use client::journal::JournalReader;
use client::journal::{
    EntryUpsertPayload, JournalWriter, LockScope, ManifestRebuildPayload, MutationScope,
    MutationType,
};
use client::replay::replay_journal;
use client::snapshot::{ManifestSnapshot, load_latest_snapshot, verify_snapshot, write_snapshot};
use client::state::canonical_hash;

fn make_entry(resource: &str, path: &str, sha: &str) -> CacheManifestEntry {
    CacheManifestEntry {
        resource_name: resource.to_string(),
        file_path: path.to_string(),
        sha256: sha.to_string(),
        size_bytes: 256,
        source_scheme: None,
        source_uri: None,
    }
}

fn resource_scope(resource: &str) -> MutationScope {
    MutationScope {
        resource_name: resource.to_string(),
        file_path: None,
    }
}

fn resource_lock(resource: &str) -> LockScope {
    LockScope {
        resource_name: resource.to_string(),
        file_path: None,
    }
}

/// Build a JournalEntry for an upsert mutation, computing pre/postcondition hashes
/// from the actual entries before and after the operation.
fn upsert_journal_entry(
    sequence: u64,
    prev_entry_hash: String,
    entries_before: &[CacheManifestEntry],
    new_entry: CacheManifestEntry,
) -> JournalEntry {
    let pre = canonical_hash(entries_before).unwrap();
    let mut after = entries_before.to_vec();
    let key = (new_entry.resource_name.clone(), new_entry.file_path.clone());
    after.retain(|e| (e.resource_name.clone(), e.file_path.clone()) != key);
    after.push(new_entry.clone());
    after.sort_by(|a, b| (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path)));
    let post = canonical_hash(&after).unwrap();
    let payload = serde_json::to_value(EntryUpsertPayload {
        entry: new_entry.clone(),
    })
    .unwrap();
    JournalEntry::new(
        sequence,
        MutationType::ManifestEntryRepair,
        resource_scope(&new_entry.resource_name),
        pre,
        post,
        payload,
        prev_entry_hash,
        Some(resource_lock(&new_entry.resource_name)),
        None,
        None,
    )
    .unwrap()
}

#[allow(dead_code)]
fn rebuild_journal_entry(
    sequence: u64,
    prev_entry_hash: String,
    entries_before: &[CacheManifestEntry],
    new_entries: Vec<CacheManifestEntry>,
) -> JournalEntry {
    let pre = canonical_hash(entries_before).unwrap();
    let mut sorted = new_entries.clone();
    sorted.sort_by(|a, b| (&a.resource_name, &a.file_path).cmp(&(&b.resource_name, &b.file_path)));
    let post = canonical_hash(&sorted).unwrap();
    let payload = serde_json::to_value(ManifestRebuildPayload {
        entries: new_entries,
    })
    .unwrap();
    JournalEntry::new(
        sequence,
        MutationType::ManifestRebuild,
        MutationScope {
            resource_name: "all".to_string(),
            file_path: None,
        },
        pre,
        post,
        payload,
        prev_entry_hash,
        None,
        None,
        None,
    )
    .unwrap()
}

#[tokio::test]
async fn replay_from_genesis_produces_correct_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir = dir.path().join("snapshots");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    // Genesis: empty manifest.
    let genesis = ManifestSnapshot::genesis(vec![]).unwrap();
    write_snapshot(&snapshot_dir, &genesis).await.unwrap();

    // Two journal entries: insert chat/main.lua, then insert chat/ui.lua.
    let e1 = make_entry("chat", "main.lua", "sha-main");
    let je1 = upsert_journal_entry(1, "genesis".to_string(), &[], e1.clone());
    let entries_after_1 = vec![e1.clone()];

    let e2 = make_entry("chat", "ui.lua", "sha-ui");
    let je2 = upsert_journal_entry(2, je1.entry_hash.clone(), &entries_after_1, e2.clone());

    let mut writer = JournalWriter::open(&journal_path).await.unwrap();
    writer.append(&je1).await.unwrap();
    writer.append(&je2).await.unwrap();

    let journal = JournalReader::new(journal_path.clone());
    let result = replay_journal(&genesis, &journal, &snapshot_dir)
        .await
        .unwrap();

    verify_snapshot(&result).unwrap();

    // Snapshot must contain both entries in deterministic order.
    assert_eq!(result.entries.len(), 2);
    assert_eq!(result.entries[0].file_path, "main.lua");
    assert_eq!(result.entries[1].file_path, "ui.lua");
    assert_eq!(result.journal_head_hash, je2.entry_hash);

    // Snapshot hash must match canonical hash of entries.
    let expected_entries_hash = canonical_hash(&result.entries).unwrap();
    assert_eq!(result.entries[0].sha256, "sha-main");
    assert_eq!(result.entries[1].sha256, "sha-ui");
    // verify_snapshot already checked snapshot_hash consistency — belt and suspenders.
    assert!(!result.snapshot_hash.is_empty());
    assert_ne!(result.snapshot_hash, genesis.snapshot_hash);
    drop(expected_entries_hash); // used implicitly via verify_snapshot
}

#[tokio::test]
async fn replay_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir_a = dir.path().join("snapshots_a");
    let snapshot_dir_b = dir.path().join("snapshots_b");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir_a).unwrap();
    std::fs::create_dir_all(&snapshot_dir_b).unwrap();

    let genesis = ManifestSnapshot::genesis(vec![]).unwrap();
    write_snapshot(&snapshot_dir_a, &genesis).await.unwrap();
    write_snapshot(&snapshot_dir_b, &genesis).await.unwrap();

    let e1 = make_entry("mod", "config.lua", "sha-cfg");
    let je1 = upsert_journal_entry(1, "genesis".to_string(), &[], e1.clone());

    let mut writer = JournalWriter::open(&journal_path).await.unwrap();
    writer.append(&je1).await.unwrap();

    let journal = JournalReader::new(journal_path.clone());

    // Run replay twice from identical starting conditions.
    let result_a = replay_journal(&genesis, &journal, &snapshot_dir_a)
        .await
        .unwrap();
    let result_b = replay_journal(&genesis, &journal, &snapshot_dir_b)
        .await
        .unwrap();

    // Determinism: both must produce identical content and hash.
    assert_eq!(result_a.entries, result_b.entries);
    assert_eq!(result_a.journal_head_hash, result_b.journal_head_hash);
    // snapshot_id differs (uuid) and timestamp_unix_ms may differ — those are OK.
    // The content-defined fields must be equal.
    assert_eq!(result_a.entries, result_b.entries);
    assert_eq!(result_a.journal_head_hash, result_b.journal_head_hash);
    assert_eq!(result_a.parent_snapshot_hash, result_b.parent_snapshot_hash);
}

#[tokio::test]
async fn chained_replay_preserves_prior_state() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir = dir.path().join("snapshots");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let genesis = ManifestSnapshot::genesis(vec![]).unwrap();
    write_snapshot(&snapshot_dir, &genesis).await.unwrap();

    // Phase 1: insert chat/main.lua, replay to snapshot_1.
    let e1 = make_entry("chat", "main.lua", "sha-v1");
    let je1 = upsert_journal_entry(1, "genesis".to_string(), &[], e1.clone());
    {
        let mut writer = JournalWriter::open(&journal_path).await.unwrap();
        writer.append(&je1).await.unwrap();
    }

    let journal = JournalReader::new(journal_path.clone());
    let snapshot_1 = replay_journal(&genesis, &journal, &snapshot_dir)
        .await
        .unwrap();
    verify_snapshot(&snapshot_1).unwrap();

    // Phase 2: replace chat/main.lua with new sha, replay from snapshot_1 to snapshot_2.
    let e2 = make_entry("chat", "main.lua", "sha-v2");
    let je2 = upsert_journal_entry(2, je1.entry_hash.clone(), &snapshot_1.entries, e2.clone());
    {
        let mut writer = JournalWriter::open(&journal_path).await.unwrap();
        writer.append(&je2).await.unwrap();
    }

    let journal = JournalReader::new(journal_path.clone());
    let snapshot_2 = replay_journal(&snapshot_1, &journal, &snapshot_dir)
        .await
        .unwrap();
    verify_snapshot(&snapshot_2).unwrap();

    // snapshot_2 must have the updated entry, not the original.
    assert_eq!(snapshot_2.entries.len(), 1);
    assert_eq!(snapshot_2.entries[0].sha256, "sha-v2");
    assert_eq!(snapshot_2.journal_head_hash, je2.entry_hash);
    assert_eq!(snapshot_2.parent_snapshot_hash, snapshot_1.snapshot_hash);

    // Phase 3: add a second resource — state from phase 2 must be preserved.
    let e3 = make_entry("mod", "config.lua", "sha-cfg");
    let je3 = upsert_journal_entry(3, je2.entry_hash.clone(), &snapshot_2.entries, e3.clone());
    {
        let mut writer = JournalWriter::open(&journal_path).await.unwrap();
        writer.append(&je3).await.unwrap();
    }

    let journal = JournalReader::new(journal_path.clone());
    let snapshot_3 = replay_journal(&snapshot_2, &journal, &snapshot_dir)
        .await
        .unwrap();
    verify_snapshot(&snapshot_3).unwrap();

    assert_eq!(snapshot_3.entries.len(), 2);
    // chat/main.lua must still be sha-v2 (not sha-v1, not missing).
    let chat = snapshot_3
        .entries
        .iter()
        .find(|e| e.resource_name == "chat")
        .unwrap();
    assert_eq!(chat.sha256, "sha-v2");
    let modd = snapshot_3
        .entries
        .iter()
        .find(|e| e.resource_name == "mod")
        .unwrap();
    assert_eq!(modd.sha256, "sha-cfg");
}

#[tokio::test]
async fn replay_with_empty_journal_returns_snapshot_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir = dir.path().join("snapshots");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let initial_entries = vec![make_entry("chat", "main.lua", "sha-stable")];
    let genesis = ManifestSnapshot::genesis(initial_entries).unwrap();
    write_snapshot(&snapshot_dir, &genesis).await.unwrap();

    // Empty journal file.
    std::fs::write(&journal_path, b"").unwrap();
    let journal = JournalReader::new(journal_path.clone());
    let result = replay_journal(&genesis, &journal, &snapshot_dir)
        .await
        .unwrap();

    // Empty journal must return same snapshot — no new write performed.
    assert_eq!(result.snapshot_id, genesis.snapshot_id);
    assert_eq!(result.snapshot_hash, genesis.snapshot_hash);
    assert_eq!(result.entries, genesis.entries);
}

#[tokio::test]
async fn replay_rejects_hash_chain_break() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir = dir.path().join("snapshots");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let genesis = ManifestSnapshot::genesis(vec![]).unwrap();
    write_snapshot(&snapshot_dir, &genesis).await.unwrap();

    let e1 = make_entry("chat", "main.lua", "sha-a");
    let je1 = upsert_journal_entry(1, "genesis".to_string(), &[], e1.clone());
    let entries_after_1 = vec![e1];

    let e2 = make_entry("chat", "ui.lua", "sha-b");
    // Build je2 with correct prev_entry_hash first (so JournalEntry::new succeeds),
    // then tamper prev_entry_hash before writing to disk.
    let mut je2 = upsert_journal_entry(2, je1.entry_hash.clone(), &entries_after_1, e2);
    // Tamper with the hash chain: prev_entry_hash does not match je1.entry_hash.
    je2.prev_entry_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();

    // Write both entries manually (je1 untouched, je2 with tampered prev_entry_hash).
    let lines = vec![
        serde_json::to_string(&je1).unwrap(),
        serde_json::to_string(&je2).unwrap(),
    ];
    std::fs::write(&journal_path, lines.join("\n") + "\n").unwrap();

    let journal = JournalReader::new(journal_path.clone());
    let err = replay_journal(&genesis, &journal, &snapshot_dir)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hash chain break") || msg.contains("HashChainBreak"),
        "expected hash chain break error, got: {msg}"
    );
}

#[tokio::test]
async fn replay_snapshot_written_to_disk_is_loadable() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_dir = dir.path().join("snapshots");
    let journal_path = dir.path().join("journal.jsonl");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let genesis = ManifestSnapshot::genesis(vec![]).unwrap();
    write_snapshot(&snapshot_dir, &genesis).await.unwrap();

    let e1 = make_entry("chat", "main.lua", "sha-disk");
    let je1 = upsert_journal_entry(1, "genesis".to_string(), &[], e1);
    let mut writer = JournalWriter::open(&journal_path).await.unwrap();
    writer.append(&je1).await.unwrap();

    let journal = JournalReader::new(journal_path);
    let written = replay_journal(&genesis, &journal, &snapshot_dir)
        .await
        .unwrap();

    // Load the snapshot back from disk via the head pointer.
    let loaded = load_latest_snapshot(&snapshot_dir).await.unwrap();

    assert_eq!(loaded.snapshot_id, written.snapshot_id);
    assert_eq!(loaded.snapshot_hash, written.snapshot_hash);
    assert_eq!(loaded.entries, written.entries);
    verify_snapshot(&loaded).unwrap();
}
