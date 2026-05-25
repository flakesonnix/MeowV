# M6.16 — State Integrity & Journal Replay

## Scope

This document is the implementation-facing specification for M6.16: durable mutation
with crash recovery.

M6.15 established bounded, inspectable mutation with a critical invariant:

> **manifest == verified committed state**

M6.16 extends this into a crash-safe transition layer. The system moves from:

- bounded mutation (M6.15)

to:

- durable mutation (M6.16)

Implemented before this document:

- M6.13 passive integrity pipeline ✅
- M6.14 mutation execution architecture ✅
- M6.15 bounded repair execution ✅

Not implemented by this document:

- distributed consensus or multi-writer coordination
- cross-machine replication
- signed manifests (reserved for M6.17+)
- Merkle chain verification (reserved for M6.17+)
- background or automatic self-healing
- orphan cleanup
- announcement-missing repair

---

## 1. Trust Model

System state spans three layers with strict precedence. Higher layers always take
priority over lower layers in case of disagreement.

### L0 — Disk State (Physical Reality)

Raw filesystem contents. Considered untrusted in isolation due to partial writes,
torn pages, and interrupted mutation. Never used as the primary decision input.

### L1 — Manifest State (Verified Snapshot)

Deterministic, validated representation of committed disk state. Produced only via:

- full scan with hash verification, or
- replay-validated journal application against a prior snapshot

**Invariant:** if manifest disagrees with disk, disk is treated as stale or corrupted.
Disk is never authoritative over manifest.

### L2 — Journal State (Intent + Mutation Log)

Append-only sequence of mutation intents. Defines how state transitions happen,
not what the final state is. Must be replayable into L1 deterministically.

### Trust Rule

> Committed truth is the last successfully validated manifest snapshot —
> not raw disk, and not the raw journal tail.

---

## 2. Core Invariants

These must hold at all times across all code paths:

1. **Journal is append-only.** No entry may be modified or deleted after write.
2. **Manifest is derived, never manually edited.** All manifest writes go through
   the journal commit path or a full scan.
3. **Disk is disposable and reconstructable.** Any disk state can be recovered by
   replaying journal entries against the last committed manifest snapshot.
4. **Every mutation is replayable.** A journal entry applied to the precondition
   state always produces the postcondition state, deterministically.
5. **No partial replay state may be persisted as valid state.** Replay is
   all-or-nothing. Intermediate in-memory state during replay is never written to disk.
6. **If uncertainty exists, prefer rollback over assumption.** When precondition or
   postcondition hashes do not match, abort and rollback.

---

## 3. Mutation Taxonomy

### Class A — Atomic Mutations

Fully applied or not applied at all. Must be idempotent. No intermediate observable
state.

Examples: single manifest entry update, single file replace, metadata patch.

Guarantees:
- safe to retry after crash
- replay does not require lock recovery
- no checkpoint state needed

### Class B — Restartable Mutations

Can be resumed from a checkpoint boundary. Must include progress markers in the
journal entry's `checkpoint_state` field.

Examples: multi-file migration, chunked copy, cache rebuild.

Guarantees:
- partial execution is detectable from checkpoint_state
- re-execution continues from last confirmed step
- full restart used only when checkpoint_state is absent or invalid

### Class C — Locked Mutations

Require exclusive access over a declared resource scope. Lock scope must be
declared in the mutation's `lock_scope` field before execution begins.

Examples: manifest rewrite, directory tree restructure, compaction passes.

Guarantees:
- serialized execution within lock scope
- no concurrent conflicting mutations permitted
- stale lock reclamation requires time + state + journal consistency check

---

## 4. Journal Schema

### Format

JSON-lines (one JSON object per line, newline-delimited). Physical append order is
logical order — no reordering permitted after write.

Constraints:
- Every entry MUST include `schema_version`.
- No non-deterministic fields (e.g. raw `Date` strings, locale-sensitive values).
- All timestamps are `unix_ms` (UTC integer milliseconds).
- `entry_id` MUST be UUIDv7 (monotonically ordered by construction).

### Entry Schema

```json
{
  "schema_version": 1,
  "entry_id": "01926f3a-7c2b-7000-8abc-0123456789ab",
  "sequence": 42,
  "timestamp_unix_ms": 1748123456789,
  "mutation_type": "manifest_entry_repair",
  "target_scope": {
    "resource_name": "chat",
    "file_path": "main.lua"
  },
  "precondition_hash": "sha256:aabbcc...",
  "postcondition_hash": "sha256:ddeeff...",
  "operation_payload": {},
  "idempotency_key": "sha256:...",
  "entry_hash": "sha256:...",
  "prev_entry_hash": "sha256:...",
  "chain_hash": null,
  "lock_scope": null,
  "checkpoint_state": null,
  "retry_count": 0,
  "origin_context": "cli:--execute-cache-repair"
}
```

### Field Semantics

| Field | Required | Notes |
|---|---|---|
| `schema_version` | yes | must be `1` for M6.16 |
| `entry_id` | yes | UUIDv7 |
| `sequence` | yes | monotonic integer, per-journal-file |
| `timestamp_unix_ms` | yes | UTC milliseconds |
| `mutation_type` | yes | see mutation type registry below |
| `target_scope` | yes | resource_name + file_path (file_path may be null for resource-level ops) |
| `precondition_hash` | yes | `sha256:<hex>` of canonical manifest JSON before apply |
| `postcondition_hash` | yes | `sha256:<hex>` of canonical manifest JSON after apply |
| `operation_payload` | yes | mutation-type-specific; schema varies per mutation_type |
| `idempotency_key` | yes | `sha256(mutation_type + canonical(target_scope) + postcondition_hash)` |
| `entry_hash` | yes | `sha256` of canonical JSON of this entry with `entry_hash` field set to `""` |
| `prev_entry_hash` | yes | `entry_hash` of preceding entry; literal string `"genesis"` for first entry |
| `chain_hash` | no | **reserved — must be `null` in M6.16**; M6.17 Merkle upgrade path |
| `lock_scope` | conditional | required for class C mutations |
| `checkpoint_state` | conditional | required for class B mutations; null for A and C |
| `retry_count` | yes | 0-indexed; incremented on each replay attempt |
| `origin_context` | no | human-readable origin; e.g. `"cli:--execute-cache-repair"` |

### Linear Hash Chain

Each entry's `entry_hash` is computed over the canonical serialization of the entry
with the `entry_hash` field replaced by `""` before hashing. The resulting chain:

```
entry[0].prev_entry_hash = "genesis"
entry[1].prev_entry_hash = entry[0].entry_hash
entry[n].prev_entry_hash = entry[n-1].entry_hash
```

This provides a tamper-detectable linear chain. `chain_hash` is reserved null in
M6.16 and will be populated with Merkle tree roots in M6.17.

### Mutation Type Registry (M6.16)

| mutation_type | class | description |
|---|---|---|
| `manifest_entry_repair` | A | upsert single manifest entry with verified cache sha |
| `file_refetch` | A | replace or populate single cache file via staged fetch |
| `manifest_rebuild` | B | full manifest reconstruction from cache scan + journal |

---

## 5. Manifest Snapshot Schema

Manifests are immutable snapshots. Each snapshot is a complete, self-contained
representation of committed state at a given journal boundary.

### Snapshot Schema

```json
{
  "schema_version": 1,
  "snapshot_id": "01926f3b-7c2b-7000-8abc-0123456789ab",
  "timestamp_unix_ms": 1748123456789,
  "journal_head_hash": "sha256:aabbcc...",
  "parent_snapshot_hash": "sha256:ddeeff...",
  "snapshot_hash": "sha256:...",
  "entries": [
    {
      "resource_name": "chat",
      "file_path": "main.lua",
      "sha256": "sha256:...",
      "size_bytes": 12345,
      "source_scheme": null,
      "source_uri": null
    }
  ]
}
```

### Field Semantics

| Field | Notes |
|---|---|
| `journal_head_hash` | `entry_hash` of the last applied journal entry; used as replay resume point |
| `parent_snapshot_hash` | `snapshot_hash` of the preceding snapshot; literal `"genesis"` for first |
| `snapshot_hash` | `sha256` of canonical JSON of this snapshot with `snapshot_hash` set to `""` |

### Content Addressability

Snapshot files MUST be stored with a filename derived from their `snapshot_id` (or
`snapshot_hash`), not a mutable name like `cache_manifest.json`. The current
canonical manifest pointer is a separate file referencing the latest snapshot_id.

This prevents silent corruption — a snapshot file whose content does not match its
filename is immediately detectable.

---

## 6. Lock Protocol

### Lease Schema

```json
{
  "schema_version": 1,
  "lock_id": "01926f3c-7c2b-7000-8abc-0123456789ab",
  "acquired_at_unix_ms": 1748123456789,
  "lease_duration_ms": 30000,
  "expires_at_unix_ms": 1748123486789,
  "lock_scope": {
    "resource_name": "chat",
    "file_path": null
  },
  "mutation_type": "manifest_rebuild",
  "precondition_hash": "sha256:aabbcc...",
  "last_journal_sequence_at_acquisition": 41,
  "writer_id": "cli-pid-12345"
}
```

### Lease Lifecycle

```
acquire → [hold → renew*] → release
              ↓ crash
         [expired] → reclaim?
```

### Reclamation Rule

A lease may be reclaimed only when ALL three conditions hold:

1. `now_unix_ms > lease.expires_at_unix_ms` (lease is expired)
2. `sha256(canonical(current_manifest)) == lease.precondition_hash`
   (manifest state is unchanged since acquisition)
3. No journal entries with `sequence > lease.last_journal_sequence_at_acquisition`
   have a `lock_scope` that overlaps with `lease.lock_scope`
   (no conflicting mutations occurred within the lock scope)

If any condition fails: full revalidation + replay of the affected scope is required
before the lock may be released or re-acquired.

**Rationale for condition 3:** manifest hash match alone is insufficient. A mutation
may have been applied, rolled back, and left the manifest at the same hash — but
journal entries within the lock scope would reveal the intervening activity.

---

## 7. Replay Algorithm

Replay is full and deterministic from the last committed snapshot. No incremental
replay from arbitrary journal positions.

```
1. Load latest manifest snapshot
   - Locate by snapshot_id pointer file
   - Verify: sha256(canonical(snapshot with snapshot_hash="")) == snapshot.snapshot_hash
   - If verification fails: locate previous snapshot and repeat

2. Load journal entries after snapshot boundary
   - Select all entries with sequence > snapshot_sequence (derived from journal_head_hash)
   - Order by sequence ascending

3. Verify linear hash chain
   - entry[0].prev_entry_hash must equal snapshot.journal_head_hash
   - For each n > 0: entry[n].prev_entry_hash must equal entry[n-1].entry_hash
   - If chain breaks at entry[k]: truncate replay to entry[k-1]

4. Apply entries in sequence
   For each entry:
   a. Verify precondition_hash matches sha256(canonical(current in-memory manifest))
   b. Apply operation_payload to in-memory manifest
   c. Verify postcondition_hash matches sha256(canonical(resulting in-memory manifest))
   d. If (a) or (c) fails: halt replay, do not persist, emit error with entry_id

5. If all entries applied successfully: write new immutable snapshot
   - Compute snapshot_hash
   - Set journal_head_hash = last applied entry's entry_hash
   - Set parent_snapshot_hash = prior snapshot's snapshot_hash
   - Write snapshot file (content-addressed)
   - Atomically update snapshot pointer

6. No partial state may be persisted at any intermediate step
   All in-memory state during replay is discarded on failure.
```

---

## 8. Failure Recovery Table

| Failure scenario | Detection | Recovery action |
|---|---|---|
| Power loss during disk write | disk content fails hash check; manifest unchanged | ignore disk; replay from last snapshot |
| Crash after journal append, before execution | entry exists; postcondition not achieved on disk | safe replay: entry treated as pending, apply normally |
| Crash mid-atomic mutation | disk hash mismatch | rollback: replay from last snapshot; idempotent reapply |
| Crash mid-restartable mutation | checkpoint_state present in entry | resume from checkpoint; full replay restart if checkpoint_state invalid |
| Crash during locked mutation | lock file present; lease expired | validate all three reclamation conditions; if any fail, full revalidation + replay |
| Manifest-journal divergence | snapshot_hash fails verification | rebuild: locate last valid snapshot; replay full journal from that boundary; disk ignored during reconstruction |
| Hash chain break at entry N | prev_entry_hash mismatch | truncate replay to entry N-1; entries after break are not applied; emit divergence error |

---

## 9. Rust Module Boundaries (Stubs)

These are the expected module boundaries, not implementations. Naming is
normative — implementations must match.

```rust
// crates/client/src/journal.rs

pub struct JournalEntry {
    pub schema_version: u32,
    pub entry_id: String,          // UUIDv7
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub mutation_type: MutationType,
    pub target_scope: MutationScope,
    pub precondition_hash: String, // "sha256:<hex>"
    pub postcondition_hash: String,
    pub operation_payload: serde_json::Value,
    pub idempotency_key: String,
    pub entry_hash: String,
    pub prev_entry_hash: String,
    pub chain_hash: Option<String>, // reserved: M6.17 Merkle
    pub lock_scope: Option<LockScope>,
    pub checkpoint_state: Option<serde_json::Value>,
    pub retry_count: u32,
    pub origin_context: Option<String>,
}

pub struct MutationScope {
    pub resource_name: String,
    pub file_path: Option<String>,
}

pub enum MutationType {
    ManifestEntryRepair,
    FileRefetch,
    ManifestRebuild,
}

pub struct JournalWriter { /* append-only handle */ }
pub struct JournalReader { /* sequential reader from sequence boundary */ }

// crates/client/src/snapshot.rs

pub struct ManifestSnapshot {
    pub schema_version: u32,
    pub snapshot_id: String,         // UUIDv7
    pub timestamp_unix_ms: u64,
    pub journal_head_hash: String,
    pub parent_snapshot_hash: String,
    pub snapshot_hash: String,
    pub entries: Vec<CacheManifestEntry>,
}

pub async fn load_latest_snapshot(snapshot_dir: &Path) -> Result<ManifestSnapshot>;
pub async fn write_snapshot(snapshot_dir: &Path, snapshot: &ManifestSnapshot) -> Result<()>;
pub async fn verify_snapshot(snapshot: &ManifestSnapshot) -> Result<()>;

// crates/client/src/lock.rs

pub struct LockLease {
    pub schema_version: u32,
    pub lock_id: String,
    pub acquired_at_unix_ms: u64,
    pub lease_duration_ms: u64,
    pub expires_at_unix_ms: u64,
    pub lock_scope: LockScope,
    pub mutation_type: MutationType,
    pub precondition_hash: String,
    pub last_journal_sequence_at_acquisition: u64,
    pub writer_id: String,
}

pub struct LockScope {
    pub resource_name: String,
    pub file_path: Option<String>, // None = entire resource
}

pub async fn acquire_lease(scope: &LockScope, config: &LeaseConfig) -> Result<LockLease>;
pub async fn release_lease(lease: &LockLease) -> Result<()>;
pub async fn try_reclaim_lease(
    lease: &LockLease,
    current_manifest: &ManifestSnapshot,
    journal: &JournalReader,
) -> Result<LeaseReclaimOutcome>;

// crates/client/src/replay.rs

pub async fn replay_journal(
    snapshot: &ManifestSnapshot,
    journal: &JournalReader,
) -> Result<ManifestSnapshot>;

pub enum ReplayError {
    PreconditionMismatch { entry_id: String, expected: String, actual: String },
    PostconditionMismatch { entry_id: String, expected: String, actual: String },
    HashChainBreak { at_sequence: u64 },
    SnapshotVerificationFailed { snapshot_id: String },
}
```

---

## 10. Upgrade Paths

### M6.17 — Merkle Chain + Signed Manifests

- Populate `chain_hash` field with Merkle tree root computed over journal segment
- `snapshot_hash` becomes a verifiable signature over known signing key
- Enables partial replay proofs and tamper-evident audit trails
- `chain_hash: null` in M6.16 is the forward-compatible placeholder

### M6.17+ — Incremental Replay Cache

- Cache partial replay results at checkpoint boundaries (class B mutations only)
- Requires checkpoint_state schema stabilization first
- Not safe to implement before Merkle chain provides integrity over cached state

### M7+ — Multi-Writer

- Requires distributed lock protocol
- Out of scope until single-writer crash safety is fully proven

---

## Excluded Scope (Explicit Non-Goals for M6.16)

- Distributed consensus
- Multi-writer conflict resolution
- Cross-machine replication
- External storage syncing
- Byzantine or adversarial fault tolerance
- Live migration between nodes
- Background or automatic self-healing
- Orphan cleanup
- Announcement-missing repair
