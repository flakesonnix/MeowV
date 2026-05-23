# Session Registry

## Purpose

`SessionRegistry` is an in-memory map that tracks every active server session.
It provides real-time aggregate counts and per-session state for local admin
and debug commands. It is never persisted to disk, never sent over a network,
and never exposed through a remote API.

## Scope

The registry is strictly local and in-process. It lives inside `SharedState`,
which is owned by `run_with_listener`. There is no file I/O, no database, no
network socket, and no telemetry sink. The registry is dropped when the server
process exits.

## Types

### `SessionId`

```rust
pub struct SessionId(u64);
```

A monotonic counter. Starts at 1 and increments by one per session. Never
based on IP addresses, player names, or any personal data. Deterministic in
tests. Implements `Copy`, `Display` (`session-N`), `Ord`.

### `SessionRegistryEntry`

| Field | Type | Description |
|---|---|---|
| `id` | `SessionId` | Unique monotonic identifier for this session. |
| `state` | `SessionState` | Current state machine state. |
| `event_count` | `usize` | Number of events recorded in the session event log. |
| `ready_dry_run` | `bool` | True when state is `ReadyDryRun`. |
| `failed` | `bool` | True when state is `Failed`. |

No IP addresses, player names, passwords, tokens, or credentials appear in
any entry field.

### `SessionRegistrySnapshot`

| Field | Type | Description |
|---|---|---|
| `connected_sessions` | `usize` | Total registered sessions (all states). |
| `ready_dry_run_sessions` | `usize` | Count with `ready_dry_run == true`. |
| `failed_sessions` | `usize` | Count with `failed == true`. |
| `sessions` | `Vec<SessionRegistryEntry>` | All entries, ordered by `SessionId` (deterministic). |

### `SessionRegistry`

In-memory map keyed by `SessionId`. Uses `BTreeMap` to guarantee deterministic
snapshot ordering.

## API

```rust
SessionRegistry::new() -> Self
create_session(&mut self) -> SessionId
update_session_state(&mut self, id: &SessionId, state: SessionState)
update_session_event_count(&mut self, id: &SessionId, event_count: usize)
update_session(&mut self, id: &SessionId, state: SessionState, event_count: usize)
remove_session(&mut self, id: &SessionId)
snapshot(&self) -> SessionRegistrySnapshot
to_diagnostics_text(&self) -> String  (on `SessionRegistrySnapshot`)
```

`update_session` combines state and event count in one lock acquisition.
`remove_session` is a no-op for unknown IDs.

## Session Lifecycle

| Point in `handle_client` | Registry call |
|---|---|
| Handler start | `create_session()` → returns `SessionId` |
| `on_hello_received()` OK | `update_session(HelloReceived, event_count)` |
| `on_version_checked()` fails | `update_session(Failed, event_count)` |
| `on_version_checked()` OK | `update_session(VersionChecked, event_count)` |
| `on_negotiation_logged()` OK | `update_session(NegotiationDryRunLogged, event_count)` |
| `on_resource_announcement_sent()` OK | `update_session(ResourceAnnouncementSent, event_count)` |
| `on_availability_report_received()` OK | `update_session(AvailabilityReportReceived, event_count)` |
| `on_policy_evaluated()` OK or PolicyBlockedDryRun | `update_session(ResourcePolicyEvaluated, event_count)` |
| `on_join_gate_sent()` OK | `update_session(JoinGateDryRunSent, event_count)` |
| `mark_ready_dry_run()` OK | `update_session(ReadyDryRun, event_count)` |
| Audit log line at session end | `update_session_event_count(final event_count)` |
| Handler exits (any path) | `SessionGuard` drops → `remove_session()` |

`SessionGuard` is a RAII guard created at handler startup. It calls
`remove_session` in its `Drop` impl, covering all exit paths including early
returns via `?`.

## Thread Safety

`SessionRegistry` is wrapped in `Arc<std::sync::Mutex<SessionRegistry>>` in
`SharedState`. All lock acquisitions are brief (no `.await` between lock and
unlock), so `std::sync::Mutex` is appropriate and deadlock-free.

The registry Arc is cloned into `admin_stdin_loop` (via `Arc<SharedState>`)
and into each `SessionGuard` instance.

## Admin Command Integration

The admin stdin loop rebuilds a `ServerRuntimeStatus` snapshot from config +
registry on every command invocation:

```
status     → shows connected/ready_dry_run/failed counts (live)
sessions   → shows connected=N ready_dry_run=N failed=N (live)
resources  → shows announcement_dir= (config-derived)
diagnostics → shows per-session diagnostics via `to_diagnostics_text()` (live)
```

The `diagnostics` command calls `SessionRegistrySnapshot::to_diagnostics_text()`
which produces deterministic multi-line output:

```
sessions: 2  ready_dry_run: 1  failed: 0
  session-1: state=Connected  events=1  ready_dry_run=false  failed=false
  session-2: state=ReadyDryRun  events=11  ready_dry_run=true  failed=false
```

No timestamps, IP addresses, or personal data appear in the output. The format
is stable across calls when session state has not changed.

## Privacy Constraints

- Session IDs are opaque monotonic integers; no IP or name embedded.
- `SessionRegistryEntry` contains no IP addresses, player names, tokens, or
  credentials.
- `SessionRegistrySnapshot` exposes only aggregate counts and state enum values.
- `handle_client` uses a separate `client_id: Uuid` for logging; the registry
  `SessionId` and the log `client_id` are unrelated and not cross-referenced
  in any output.

## Relation to Other Components

| Component | Scope |
|---|---|
| `SessionRegistrySnapshot::to_diagnostics_text` | Deterministic diagnostics text for admin display. |
| `ServerRuntimeStatus` | Config-derived snapshot; gains live counts from registry. |
| `SessionRegistry` | Server-wide aggregate; live session counts and states. |
| `SessionDiagnostics` | Per-session snapshot: state history + event log. |
| `SessionEventLog` | Per-session ordered event record; local to handler task. |
| Structured logs | Per-event chronological stream to stdout. |

## Future: Persistent Metrics

A future milestone could optionally export session counts to a local metrics
file or expose them through a local socket for monitoring. Any such feature
would require its own milestone, explicit config gating, and a security review.
No such feature is planned or implemented here.

## Hard Boundaries

This feature does not and will not:

- Write registry data to disk.
- Expose a network socket for registry queries.
- Include client IP addresses or personal data.
- Add telemetry, metrics export, or log aggregation.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All registry data is local, ephemeral, and limited to the server process lifetime.
