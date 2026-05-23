# Resource Cache Repair Plan

## Purpose

Produce a deterministic, report-only repair plan from cache verification results. No downloads, no filesystem mutation, no execution.

## Types

### `CacheRepairAction`

```rust
pub enum CacheRepairAction {
    None,            // Cache entry is valid — no action needed
    FetchMissing,    // File is missing from cache — needs download
    ReplaceInvalid,  // File exists but is corrupted (size or hash mismatch)
    VerifyOnly,      // Reserved for future use (exists but unverified)
}
```

### `CacheRepairPlanEntry`

One entry per file in the verification report:

| Field | Type | Source |
|-------|------|--------|
| `relative_path` | `PathBuf` | from `CacheVerificationEntry` |
| `expected_size_bytes` | `u64` | from `CacheVerificationEntry` |
| `expected_sha256` | `String` | from `CacheVerificationEntry` |
| `action` | `CacheRepairAction` | derived from `CacheFileStatus` |

### `CacheRepairPlan`

Aggregate plan for one resource:

| Field | Type |
|-------|------|
| `entries` | `Vec<CacheRepairPlanEntry>` |
| `fetch_missing_count` | `usize` |
| `replace_invalid_count` | `usize` |
| `verify_only_count` | `usize` |
| `noop_count` | `usize` |

Methods:
- `is_noop()` — `true` when `fetch_missing_count == 0 && replace_invalid_count == 0`
- `to_text()` — deterministic multi-line human-readable output

## Mapping

| `CacheFileStatus` | `CacheRepairAction` |
|---|---|
| `Valid` | `None` |
| `Missing` | `FetchMissing` |
| `SizeMismatch` | `ReplaceInvalid` |
| `HashMismatch` | `ReplaceInvalid` |

## Pure Function

```rust
pub fn build_cache_repair_plan(report: &CacheVerificationReport) -> CacheRepairPlan
```

- Takes `&CacheVerificationReport` — no filesystem access, no I/O
- Iterates entries in report order (already deterministic from `verify_cache_against_index`)
- Counts action types independently
- No sorting, no allocation beyond the plan itself

## Client CLI

```
cargo run --bin meowv-client -- --plan-cache-repair <resource_dir> <cache_dir>
```

- Builds pack index from `resource_dir`
- Verifies cache against index
- Prints repair plan
- Does **not** download, modify, or execute any files
- Exits after printing

## Example Output

```
Cache Repair Plan: 3 entries
  Fetch Missing: 1
  Replace Invalid: 1
  Verify Only: 0
  No Action: 1
  client/main.lua -> fetch (14 bytes, abc123...)
  server/main.lua -> replace (12 bytes, def456...)
  resource.toml -> noop (200 bytes, 789ghi...)
Cache is fully valid, no repair needed.
```

## No-Execution Guarantee

- `build_cache_repair_plan` is pure data transformation: `&CacheVerificationReport → CacheRepairPlan`
- Client `--plan-cache-repair` is inspect-only: `build_pack_index → verify_cache_for_resource → build_cache_repair_plan → print → exit`
- No file writes, no downloads, no script execution, no network access
- All existing boundaries preserved: no remote, no persistence, no telemetry

## Security

- Same path validation as cache verification — symlinks rejected, absolute paths rejected, `..` traversal rejected
- Plan is derived deterministically from already-verified report — no new attack surface
- Plan actions are advisory only; real repair requires separate implementation milestone
