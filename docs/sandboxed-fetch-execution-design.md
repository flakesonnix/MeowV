# Sandboxed Fetch Execution Design

## Scope

This document is a **future-only design specification**. No fetching, network
I/O, cache writing, or resource execution is implemented in the current
milestone. Source metadata validation (M6.5), source selection (M6.6), source
policy reporting (M6.7), and fetch execution planning (M6.9) exist as pure,
report-only preflight steps.

The next implementation milestone after this design is M6.10 (staged fetch
implementation behind an explicit opt-in flag) and M6.11 (cache commit /
atomic move after verification). M6.9 (fetch execution planner) is already
implemented.

## Design Goals

1. **Sandboxed**: fetched bytes touch only a temp staging directory until
   verified; the cache is updated only after SHA-256 and policy checks pass.
2. **Verifiable**: every byte is checked against the announced hash before
   becoming visible to any resource loader.
3. **Non-executing**: fetched resources are never executed, sourced, or
   interpreted during or after fetch.
4. **Observable**: every phase can be inspected via dry-run, logs, or JSON
   report; no secret or personal data leaks.
5. **Opt-in**: fetching requires explicit configuration; silent or automatic
   downloads never occur.

## Fetch Phases

### Phase 1 — Resolve

- Use the `selected_source` from the preflight entry (M6.6).
- Validate that the source still passes source policy (M6.7).
- If no valid source exists, the fetch is aborted with a `NoValidSource`
  error.
- If the source policy decision is `Blocked`, the fetch is aborted with a
  `BlockedBySourcePolicy` error.

### Phase 2 — Fetch to Staging

- Create a temp staging path under a configured staging directory (e.g.,
  `<cache_dir>/.staging/`).
- Stream from the resolved URL to the staging path.
- Enforce:
  - Max size limit (per-file `size_bytes` from announcement, plus a
    configurable overhead buffer).
  - Timeout limit (configurable per-request).
  - Redirect limit (configurable; default 0 = no redirects).
  - Allowed schemes: `https`, optionally `file` (local), optionally `ipfs`
    (requires gateway).
- Reject symlinks, device nodes, and special files in the staging directory.
- Reject path traversal in the staging path.
- No archive extraction, no content inspection, no execution.

### Phase 3 — Verify

- Compute SHA-256 of the staged file.
- Compare against `sha256` from the announced file metadata.
- If mismatch:
  - Delete the staging file.
  - Record a `HashMismatch` error.
  - Do not proceed to cache commit.
- If match, proceed.

### Phase 4 — Policy Gate (optional)

- If strict signature policy is enabled, verify the announcement signature
  before allowing cache commit.
- If resource policy evaluation blocks the resource, do not commit.

### Phase 5 — Cache Commit

- Atomically rename the staged file into the cache directory at the path
  `cache_dir/<resource_name>/<relative_path>`.
- Use `std::fs::rename` (atomic on same filesystem) or copy-then-delete if
  cross-filesystem.
- On rename failure:
  - Delete the staging file.
  - Record a `CacheCommitFailed` error.
  - Do not leave partial files in the cache.

### Phase 6 — Observe

- After commit, rebuild the cache verification report for the affected
  resource to confirm availability.
- Log the successful fetch at `info` level (resource, file, size, source
  scheme, no secrets).
- Emit structured log line with: resource name, file path, size, outcome,
  source scheme.

## Sandbox Boundaries

### Filesystem

| Operation | Allowed Path | Denied Path |
|-----------|-------------|-------------|
| Temp write | `<cache_dir>/.staging/*` | Any path outside staging |
| Atomic rename | `<cache_dir>/<resource>/<file>` | System dirs, symlink targets |
| Read during verify | Staging file only | Cache dir before verify |
| Delete on failure | Staging file only | Anything outside staging |
| Symlink creation | Never | Everywhere |

### Network

| Property | Default | Configurable |
|----------|---------|-------------|
| Allowed schemes | `https` | `file`, `ipfs` via source policy |
| Max request time | 30 s | Yes |
| Max response bytes | `file.size_bytes * 1.1` | Yes |
| Max redirects | 0 (none) | Yes |
| User-Agent | `MeowV/<version>` | Yes |
| Credential forwarding | Never | N/A |

### Execution

- Staged files are never executed, sourced, or loaded as scripts.
- Staged files are never passed to any interpreter (Lua, JS, WASM, shell).
- Staged files are never mmap'd as executable memory.

## Failure Modes

| Failure | Phase | Handling |
|---------|-------|----------|
| No valid source | Resolve | Abort fetch; record `NoValidSource` error |
| Blocked by policy | Resolve | Abort fetch; record `BlockedBySourcePolicy` error |
| Connection failed | Fetch | Retry up to N times (configurable); record `FetchFailed` |
| Timeout | Fetch | Abort; record `Timeout` |
| Size exceeded | Fetch | Abort; record `SizeExceeded`; delete partial staging file |
| Too many redirects | Fetch | Abort; record `RedirectLimitExceeded` |
| Unsupported scheme | Fetch | Abort; record `UnsupportedScheme` |
| Hash mismatch | Verify | Delete staging; record `HashMismatch` |
| Staging write error | Fetch | Abort; record `StagingWriteFailed` |
| Atomic rename failure | Commit | Delete staging; record `CacheCommitFailed` |
| Temp directory creation failure | Fetch | Abort; record `TempDirFailed` |

## Observability

### Dry-Run

- Existing `--plan-resource-downloads` preflight output already shows the
  source that would be used (selected_source), its policy status, and the
  preflight action (FetchMissing or ReplaceInvalid).
- A future `--dry-run` flag on the actual fetch command can show exactly what
  would happen without doing it.

### Logs

- `info!` on successful fetch: resource, file, size, scheme.
- `warn!` on retry: attempt number, error.
- `error!` on failure: failure mode, resource, file, error details.
- No secrets, tokens, or credentials in any log output.

### JSON Report

- An optional `--fetch-report <path>` flag can write a structured JSON report
  for each attempted fetch:
  - resource_name, file_path, source_scheme, source_uri (scheme+host only,
    no query/path secrets)
  - outcome: `success`, `failure`
  - failure_reason (if failed)
  - size_bytes, sha256 (from announcement)
  - duration_ms

## Relationship to Existing Milestones

- **M6.5**: Source metadata validation (scheme, hash, size, duplication).
  Preflight prerequisite for any fetch.
- **M6.6**: Source selection (priority/id/uri). Determines which source would
  be used.
- **M6.7**: Source policy reporting. Determines whether the selected source
  is allowed.
- **M6.8**: This design doc. No implementation.
- **M6.9**: Fetch execution planner — pure/no I/O planning for a specific
  source URL, staging path, and verification step.
- **M6.10**: Staged fetch implementation — actual HTTP/file/ipfs fetching
  behind an explicit opt-in flag.
- **M6.11**: Cache commit — atomic move after verification, cache state
  update, observability.

## Future Milestone Details

### M6.9 — Fetch Execution Planner, Pure/No I/O

- Add a pure planner that takes a preflight entry + selected source and
  produces a structured fetch plan with explicit staging path, verification
  steps, and expected outcomes.
- Report-only: no network, no writes, no execution.
- Output can be rendered as text or JSON for dry-run inspection.
- No dependencies added.

### M6.10 — Staged Fetch Implementation

- Implement fetch behind an explicit opt-in CLI flag (e.g.,
  `--allow-fetch`).
- Enforce all sandbox boundaries defined in this design.
- Support `https` scheme initially; `file` and `ipfs` gated behind policy.
- Enforce size, timeout, and redirect limits.
- Record fetch outcomes in a structured report.
- No cache commit yet — fetch stops after verification.
- Requires at least one integration test with a local file:// source.

### M6.11 — Cache Commit / Atomic Move

- Implement atomic rename from staging to cache after successful
  verification.
- Update cache verification state so subsequent preflight runs see the file
  as `Available`.
- Record commit outcomes in structured report.
- Delete staging file on success.
- Roll back on failure (delete staging, leave cache unchanged).

## Hard Boundaries

- Fetch never executes resources.
- Fetch never writes outside staging or cache directories.
- Fetch never follows unsafe redirects by default.
- Fetch never forwards credentials or tokens.
- Fetch never extracts archives.
- Fetch never runs post-fetch hooks or callbacks.
- Fetch never modifies the announcement, policy, or session state.
- Cache commit is the only path from staging to cache, and it requires
  successful verification.

## Implementation Status

### M6.9 — Fetch Execution Planner ✅

- `ResourceFetchExecutionStep` enum with 8 variants covering resolve, stage,
  verify, commit, and blocked states.
- `ResourceFetchExecutionEntry` — per-file entry with `plan_ok`, `steps`,
  `block_reason`.
- `ResourceFetchExecutionPlan` — aggregate plan with `to_text()` output.
- `build_fetch_execution_plan(preflight)` — pure deterministic planner from
  preflight data. No I/O, no network, no cache writes, no execution.
- 14 unit tests covering all action types, blocked states, text/JSON output,
  determinism, and purity guarantees.
- 183 protocol tests / 541 workspace tests total.
