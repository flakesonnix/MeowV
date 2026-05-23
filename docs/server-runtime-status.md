# Server Runtime Status Snapshot

## Purpose

`ServerRuntimeStatus` is a compact in-memory snapshot of current server state.
It can be produced on demand for local debug inspection — powering the admin
`status`, `sessions`, and `resources` commands. It is never serialized to disk,
never sent over a network, and never exposed through a remote API.

## Scope

This is strictly a local, in-process debug tool. There is no web panel, no
remote admin socket, no telemetry sink, and no persistent log file. The snapshot
is generated when an admin command is issued and discarded immediately after
printing. It does not persist between commands.

## Fields

| Field | Type | Source | Description |
|---|---|---|---|
| `server_name` | `String` | config | Human-readable server name from `[server].name`. |
| `bind_addr` | `String` | config | Listen address from `[server].bind_addr`. Server address only; no client IPs. |
| `protocol_version` | `u32` | compile-time const | `PROTOCOL_VERSION` constant from the protocol crate. |
| `exact_version_required` | `bool` | config | `[protocol].exact_version_required`. |
| `negotiation_dry_run` | `bool` | config | `[protocol].negotiation_dry_run`. |
| `capability_gates_report_only` | `bool` | config | `[protocol].capability_gates_report_only`. |
| `join_gate_mode` | `String` | config | `"dry_run"` (only supported mode). |
| `connected_sessions` | `usize` | live/default | Connected session count. Defaults to `0`; future milestones will wire live state. |
| `ready_dry_run_sessions` | `usize` | live/default | Sessions that reached `ReadyDryRun`. Defaults to `0`. |
| `failed_sessions` | `usize` | live/default | Sessions that reached `Failed`. Defaults to `0`. |
| `resource_announcement_dir` | `String` | config | `[resources].announcement_resource_dir`. |
| `diagnostics_enabled` | `bool` | config | `[diagnostics].print_session_diagnostics`. |
| `admin_stdin_enabled` | `bool` | config | `[admin].local_stdin_enabled`. |

## Construction

```rust
let status = ServerRuntimeStatus::from_config(&config);
```

Derives all fields from the server config. Session counts default to zero.
Use `with_session_counts(connected, ready_dry_run, failed)` to return an
updated snapshot once live session tracking is wired.

## Text Output

`to_text()` returns a deterministic newline-separated key: value string:

```
server_name: MeowV Local Dev Server
bind_addr: 127.0.0.1:7000
protocol_version: 1
exact_version_required: true
negotiation_dry_run: true
capability_gates_report_only: true
join_gate_mode: dry_run
connected_sessions: 0
ready_dry_run_sessions: 0
failed_sessions: 0
resource_announcement_dir: examples/resources/chat
diagnostics_enabled: true
admin_stdin_enabled: false
```

Output is deterministic: given the same config and counts, `to_text()` always
returns the same string. No timestamps. No random or runtime-varying fields.

## Admin Command Integration

The admin stdin loop builds a `ServerRuntimeStatus` snapshot from config at
startup and passes it to `handle_admin_command_with_status`:

| Command | Behaviour with snapshot |
|---|---|
| `status` | Prints full `to_text()` output. |
| `sessions` | Prints `connected=N ready_dry_run=N failed=N`. |
| `resources` | Prints `announcement_dir=<path>`. |
| `diagnostics` | Placeholder (live diagnostics not yet wired). |
| `help` | Lists commands (no snapshot data used). |
| `quit` | Signals server shutdown (no snapshot data used). |

`handle_admin_command` (no snapshot) falls back to the generic placeholder
strings for all commands that would otherwise show snapshot data.

## Privacy Constraints

- `bind_addr` is the server's own listen address, not any client address.
- No client IP addresses, peer addresses, or remote addresses appear in any
  field or in the `to_text()` output.
- No player names, session tokens, or credentials appear in the snapshot.
- Session counts are aggregate integers; no per-session identity is exposed.

## Relation to Session Diagnostics and Event Log

| Component | Scope |
|---|---|
| `ServerRuntimeStatus` | Server-wide aggregate. Config-derived + optional live counts. |
| `SessionDiagnostics` | Per-session snapshot: state history, event log, policy decisions. |
| `SessionEventLog` | Per-session ordered event record. In-memory, local to handler task. |
| Structured logs (`tracing`) | Per-event chronological stream to stdout. |

These components complement each other. `ServerRuntimeStatus` gives a bird's-eye
view; `SessionDiagnostics` gives per-session depth.

## Future: Live Session Registry

Session counts in `ServerRuntimeStatus` currently default to zero. Wiring live
counts requires a shared session registry (e.g., an `Arc<RwLock<SessionMap>>`)
visible to both `handle_client` tasks and the admin snapshot path. That registry
will be added in a future milestone. When available, `with_session_counts` will
be called with real values.

## Hard Boundaries

This feature does not and will not:

- Expose a network socket or remote API for status queries.
- Write status data to disk.
- Include client IP addresses or personal data.
- Add telemetry, metrics export, or log aggregation.
- Include timestamps or non-deterministic fields.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All status data is local, ephemeral, and derived from config or in-process state.
