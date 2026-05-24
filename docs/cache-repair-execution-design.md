# Cache Repair Execution Design

## Scope

This document is a design-only specification for the first mutation step after
passive cache integrity milestones M6.9 through M6.13.

This is the hard boundary where the cache system changes from:

- observational
- report-only
- non-mutating beyond explicit fetch+commit

to:

- repair-capable
- deletion-adjacent
- operationally state-changing

Implemented before this design:
- M6.9 fetch execution planner ✅
- M6.10 staged fetch implementation ✅
- M6.11 verified cache commit ✅
- M6.12 cache metadata manifest ✅
- M6.13 cache reconciliation planner ✅

Not implemented by this document:
- no repair executor
- no orphan deletion
- no background reconciliation
- no automatic self-healing
- no runtime-triggered repair hooks

## Problem Statement

The cache subsystem can now do all passive integrity work:

1. plan fetches
2. fetch into staging
3. verify announced hashes
4. commit verified files into cache
5. record committed state in `cache_manifest.json`
6. reconcile manifest, filesystem, and announcement state

What does not exist yet is a safe, explicit mutation path from a
`CacheReconciliationPlan` to actual cache repair operations.

That transition is operationally dangerous because it introduces:
- deletion
- replacement of cached files
- manifest repair
- partial failure handling
- crash recovery requirements
- concurrency and locking requirements

This design defines those semantics before any repair code is written.

## Design Goals

1. **Explicit opt-in**: repair must never run automatically.
2. **Planner-derived**: execution must only perform operations justified by a
   deterministic reconciliation plan.
3. **Atomic where possible**: file replacement and manifest updates should use
   staging and rename semantics, not in-place mutation.
4. **Crash-safe**: interruption must leave the cache explainable and
   recoverable by a subsequent reconciliation pass.
5. **Idempotent**: rerunning repair after partial success should converge
   toward the same final state.
6. **Observable**: every intended and actual mutation must be visible in text
   and JSON reports.
7. **Separated mutation classes**: file repair and orphan cleanup should not be
   introduced in the same execution milestone.

## Non-Goals

- automatic repair during login or runtime handshake
- background cache daemons
- implicit repair on reconciliation
- manifest-only blind trust without filesystem verification
- archive extraction, execution, or runtime activation
- cross-process distributed locking

## Current Inputs

The future repair executor will depend on existing data sources:

- `CacheManifest` from `crates/client/src/fetch.rs`
- `CacheReconciliationPlan` from `crates/client/src/reconciliation.rs`
- `ResourceAnnouncement` as announcement truth
- fetch execution pipeline for re-fetching missing or mismatched files

The current reconciliation action set is:

- `AlreadyConsistent`
- `MissingManifestEntry`
- `MissingCacheFile`
- `HashMismatch`
- `OrphanedCacheFile`
- `ManifestCorrupted`
- `AnnouncementMissing`
- `WouldRepairManifest`
- `WouldRemoveOrphan`
- `WouldRefetch`

Only the first seven are currently emitted by M6.13. The `Would*` variants are
reserved for future execution and preview flows.

## Repair Model

Repair must remain a two-step model:

1. **Reconcile**: derive state inconsistencies without side effects.
2. **Execute**: apply an explicit operator-approved mutation plan.

Execution must never infer new mutations from raw filesystem state on its own.
It must consume a deterministic plan or rebuild that same plan immediately
before execution and verify parity.

Recommended invariant:

- if a fresh reconciliation pass differs from the reviewed dry-run plan, repair
  execution should stop and require operator re-approval

## Proposed Milestone Sequencing

- **M6.14**: repair execution design only
- **M6.15**: repair plan executor for refetch and manifest repair only
- **M6.16**: orphan cleanup executor
- **M6.17**: optional hooks/integration points, still explicit opt-in

This sequencing intentionally keeps deletion separate from refetch/replace
operations.

## Repair Action Classes

### Class A: Manifest Repair

Derived from:
- `MissingManifestEntry`
- possibly `ManifestCorrupted`

Safe mutation shape:
- rebuild or insert manifest metadata for cache files that already exist and
  match announcement expectations

Risk level:
- low to medium

### Class B: File Refetch / Replace

Derived from:
- `MissingCacheFile`
- `HashMismatch`

Safe mutation shape:
- fetch to staging
- verify
- atomic rename into cache
- update manifest after successful commit

Risk level:
- medium

### Class C: Orphan Cleanup

Derived from:
- `OrphanedCacheFile`

Safe mutation shape:
- explicit deletion only after dedicated policy gate

Risk level:
- high

### Class D: Manifest Recovery

Derived from:
- `ManifestCorrupted`

Safe mutation shape:
- reconstruct a fresh manifest from files that are both present in cache and
  present in the current announcement, optionally after verification rules are
  satisfied

Risk level:
- medium

### Class E: Announcement-Missing Cases

Derived from:
- `AnnouncementMissing`

Default behavior:
- report only

Rationale:
- if the current announcement no longer references a manifest-tracked file, that
  may reflect version drift, not necessarily corruption. Automatic deletion or
  replacement is unsafe here.

## Operator Controls

Repair execution must require explicit operator intent.

Recommended future CLI shape:

```text
client --execute-cache-repair <announcement.json> --resource-cache <path>
```

Recommended operator flow:

1. generate reconciliation plan
2. inspect dry-run repair plan
3. rerun with explicit execution flag
4. inspect repair report

Required gates:

- explicit execution flag distinct from reconcile/planning flags
- explicit cache directory
- explicit announcement input
- explicit fetch permission if network/file refetch is required
- explicit orphan cleanup permission separate from general repair

Recommended additional flags:

- `--dry-run`
- `--allow-manifest-repair`
- `--allow-refetch-repair`
- `--allow-orphan-delete`
- `--trusted-keys <path>` when strict signature policy applies
- `--signature-policy strict|report_only`

## Dry-Run Parity

Execution must have exact dry-run parity.

That means a future execution command should support:

1. producing the exact action set it would perform
2. rendering the same action set in text and JSON
3. performing no writes or deletions under dry-run

Dry-run and execution should differ only at the final mutation boundary.

## Atomicity Rules

### File Replacement

For missing or mismatched files:

1. fetch into `.staging/`
2. verify hash against announcement
3. atomically rename into cache target path
4. update manifest atomically after file commit succeeds

Never:
- overwrite cache files in place
- edit files in place
- update manifest before verified file commit succeeds

### Manifest Update

Manifest writes should remain atomic using temp file + rename.

For multi-file repair runs, the design choice is:

- perform manifest updates per successful file commit, or
- accumulate manifest state in memory and write once at the end

Recommended rule for M6.15:

- update manifest after each successful file mutation

Rationale:
- simpler crash semantics
- lower rollback complexity
- preserves accurate committed-state projection after each successful step

### Orphan Deletion

Deletion should not be mixed into the first repair executor milestone.

When introduced later, deletion should be:

1. explicit opt-in
2. single-file scoped in reports
3. non-recursive by default per planned file path
4. followed by manifest reconciliation if relevant

## Rollback Semantics

Rollback should be limited and local.

Recommended rule:

- if staging fetch or verification fails, delete staging artifact only
- if atomic rename fails, delete staging artifact only
- if manifest write fails after a successful file commit, do not roll back the
  file mutation; instead report a manifest-sync failure and rely on subsequent
  reconciliation to surface the drift

Rationale:
- file rollback after successful rename can itself fail and increase risk
- the system already has a passive reconciliation layer that can explain this
  mismatch safely

## Partial Failure Handling

Repair runs may contain multiple entries. Failure handling must therefore be
per-entry, not transaction-global.

Recommended behavior:

- execute entries independently in deterministic order
- record success/failure per entry
- continue past entry-local failures unless a fatal invariant is violated

Fatal invariant examples:
- cache directory lock cannot be acquired
- announcement input is invalid
- strict signature requirements fail before any mutation begins

Non-fatal entry-local failures:
- single file fetch timeout
- hash mismatch on refetch
- individual manifest write failure
- per-entry path traversal rejection

Recommended entry ordering:

- deterministic lexical order by `(resource_name, file_path)`
- no parallel mutation in first executor milestone

Rationale:

- simpler logs and reasoning
- simpler crash recovery
- easier test determinism
- no lock contention inside a single repair run

## Crash Recovery

Crash recovery is one of the main reasons repair must be staged.

Required properties:

- `.staging/` remains disposable and ignorable by reconciliation
- partially downloaded staging files must never be treated as committed cache
  state
- manifest temp files must never be treated as authoritative state
- rerunning reconciliation after a crash must produce an explainable plan

Expected post-crash outcomes:

- if crash occurs before rename: cache unchanged; stale staging may remain
- if crash occurs after rename but before manifest update: cache file exists,
  manifest may lag; reconciliation should report `MissingManifestEntry`
- if crash occurs during manifest temp write: last committed manifest remains
  authoritative

## Manifest Synchronization Order

Manifest synchronization must follow this order:

1. verify source bytes
2. commit file into cache
3. write manifest entry reflecting committed file

Never:

1. write manifest before cache commit
2. delete manifest entries before corresponding cache mutation semantics are
   complete

For manifest rebuild after corruption:

- reconstruct only from currently committed cache files that are admitted by the
  current announcement scope
- sort deterministically by `(resource_name, file_path)`
- write rebuilt manifest atomically

## Staged vs Direct Repair

Direct repair is prohibited for file content changes.

Allowed:
- staged fetch
- verify
- atomic rename

Not allowed:
- truncate-and-rewrite cache files in place
- direct overwrite of mismatched files
- direct mutation of target files before verification

Manifest-only repair may write directly via the existing atomic manifest temp
write path, because the current implementation already treats manifest writes as
staged writes.

## Concurrency Model

The first repair executor should assume single-process ownership of a cache
directory during repair.

Recommended future behavior:

- acquire a cache repair lock scoped to the cache directory before mutation
- refuse to start if another repair process holds the lock
- treat reconciliation-only reads as lock-free

Recommended lock properties:

- local filesystem lock only
- best-effort stale lock recovery documented explicitly
- no distributed or cross-host coordination

This design does not prescribe the exact locking primitive yet, only the
requirement that mutation be serialized per cache directory.

Recommended first implementation bias:

- single-threaded repair executor
- one cache directory per process
- no concurrent manifest writers

## Idempotency

Repair should be safely rerunnable.

Examples:

- rerunning manifest repair after a prior partial success should produce either
  no-op or the same final manifest
- rerunning refetch after a completed successful repair should become
  `AlreadyConsistent`
- rerunning after manifest write failure should repair only the manifest drift

This is another reason repair must be derived from reconciliation, not implicit
mutation heuristics.

## Reporting Model

Future execution should have a structured report parallel to fetch reporting.

Recommended entry fields:

- `resource_name`
- `file_path`
- `planned_action`
- `executed_action`
- `outcome`
- `failure_reason`
- `duration_ms`
- `manifest_outcome`
- `fetch_outcome` when refetch is involved

Recommended high-level report fields:

- total entries
- succeeded
- failed
- skipped
- manifest_corrupted_input
- dry_run

Recommended outcome categories:

- `repaired`
- `skipped`
- `failed`
- `blocked`

This should mirror the current planner/report style: deterministic, compact,
and explainable without reading implementation code.

## Orphan Cleanup Policy

Orphan cleanup must be conservative.

Recommended policy for first orphan-deletion milestone:

- only delete files explicitly classified as `OrphanedCacheFile`
- never delete `AnnouncementMissing` entries automatically
- never delete directories recursively as a primary action
- delete only the exact planned file path
- allow empty parent directories to remain

This keeps deletion semantics narrow and auditable.

## Announcement Scope and Trust

Repair execution is scoped to a specific announcement input.

Implications:

- a repair run should only mutate files justified by the provided announcement
- a stale or different announcement can change repair classifications
- strict signature policy must be enforced before any mutation when required

Therefore execution should report the announcement identity context it used,
including protocol/signature policy inputs where applicable.

## Safe First Execution Boundary

The safest first mutation milestone is:

- manifest repair
- refetch for `MissingCacheFile`
- refetch for `HashMismatch`

Explicitly excluded from the first execution milestone:

- orphan deletion
- automatic handling of `AnnouncementMissing`
- background repair hooks
- runtime-coupled repair invocation

## Recommended M6.15 Scope

Implement a repair executor with these constraints:

1. consumes reconciliation-derived actions only
2. supports dry-run parity
3. repairs `MissingCacheFile` via existing staged fetch pipeline
4. repairs `HashMismatch` via existing staged fetch pipeline
5. repairs `MissingManifestEntry` by atomic manifest insertion only when cache
   file matches announcement
6. supports manifest rebuild after `ManifestCorrupted`
7. does not delete orphan files
8. does not act on `AnnouncementMissing`

## Recommended Tests for First Executor Milestone

- repair dry-run matches execution plan shape
- missing file repaired via fetch+verify+commit
- hash mismatch repaired via fetch+verify+replace
- manifest entry repaired without file mutation when cache file matches
- corrupted manifest rebuilt atomically
- manifest write failure leaves file commit intact and is reported
- stale `.staging/` ignored before and after repair run
- rerun after partial success is idempotent
- repair refuses to start without explicit opt-in
- repair refuses orphan deletion without dedicated flag

## Hard Boundaries

- repair must never execute resources
- repair must never mutate cache without explicit operator opt-in
- repair must never delete files in M6.15
- repair must never trust manifest state over filesystem+verification blindly
- repair must never write outside cache directory and staging directory
- repair must never run automatically during handshake, login, or runtime flow

## Summary

M6.13 completed the passive integrity loop. The next step must not be direct
implementation of mutation logic. The correct next move is a narrow,
deterministic, report-first repair executor design that preserves:

- explainability
- deterministic ordering
- crash recoverability
- operator control
- strict separation between observation and mutation

## Related Documents

- `docs/sandboxed-fetch-execution-design.md` — staged fetch, verified commit,
  and manifest semantics through M6.13
- `docs/resource-download-preflight.md` — report-only source planning and
  reconciliation context
- `crates/client/src/fetch.rs` — existing fetch/commit/manifest primitives
- `crates/client/src/reconciliation.rs` — passive reconciliation planner
