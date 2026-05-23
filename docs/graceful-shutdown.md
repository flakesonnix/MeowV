# Graceful Shutdown

## Purpose

The MeowV server supports a graceful shutdown flow triggered by the local admin
`quit` command. When shutdown is requested, the server stops accepting new
connections, snapshots its final runtime state and session registry, and logs a
deterministic shutdown summary. There is no remote shutdown API, no persistence,
and no telemetry.

## Types

### `ShutdownReason`

```rust
pub enum ShutdownReason {
    AdminQuit,
    InternalError,
    TestRequested,
}
```

| Variant | Meaning |
|---|---|
| `AdminQuit` | Local admin `quit` command. |
| `InternalError` | Reserved for future internal-error-triggered shutdown. |
| `TestRequested` | Reserved for test-driven shutdown scenarios. |

Implements `Display` for logging (`admin_quit`, `internal_error`,
`test_requested`).

### `ShutdownState`

```rust
pub struct ShutdownState { ... }
```

| Method | Description |
|---|---|
| `new()` | Creates a new state with no shutdown requested. |
| `request(reason)` | Requests shutdown. First call wins; subsequent calls are no-ops. |
| `is_requested()` | Returns `true` if shutdown has been requested. |
| `reason()` | Returns `Option<ShutdownReason>`. |

All methods are deterministic. No I/O, no timestamps, no external state.

### `ShutdownSummary`

```rust
pub struct ShutdownSummary {
    pub reason: ShutdownReason,
    pub status_dump: String,
    pub registry_dump: String,
}
```

- `reason`: the reason shutdown was requested.
- `status_dump`: output of `ServerRuntimeStatus::to_text()` at shutdown time.
- `registry_dump`: output of `SessionRegistrySnapshot::to_diagnostics_text()` at
  shutdown time.

### `build_shutdown_summary`

```rust
pub fn build_shutdown_summary(
    config: &ServerConfig,
    registry_snapshot: &SessionRegistrySnapshot,
    reason: ShutdownReason,
) -> ShutdownSummary
```

Uses existing `ServerRuntimeStatus::from_config` and
`SessionRegistrySnapshot::to_diagnostics_text` internally. Deterministic: same
inputs always produce identical output.

## Integration

### SharedState

`SharedState` holds `shutdown: std::sync::Mutex<ShutdownState>` as a third
field alongside `clients` and `registry`. The mutex is appropriate because lock
duration is brief (no `.await` held).

### admin_stdin_loop

When the admin `quit` command is parsed and produces `should_quit: true`, the
loop first calls `state.shutdown.lock().unwrap().request(AdminQuit)`, then
sends the oneshot quit signal and returns.

### run_with_listener

After the `tokio::select!` between `accept_loop` and `quit_rx` resolves, the
server builds a `ShutdownSummary` and logs it at `info` level:

```
INFO server shutdown: final summary
--- status ---
server_name: ...
bind_addr: ...
protocol_version: ...
...
--- sessions ---
sessions: 0
(no active sessions)
```

The summary contains:
- Shutdown reason
- Full runtime status snapshot (server identity, policy flags, session counts)
- Registry diagnostics snapshot (per-session IDs, states, event counts)

No IP addresses, personal data, or timestamps appear in the summary.

### Accept Loop

The accept loop stops via the existing oneshot quit channel. When `quit_rx`
fires in the `tokio::select!`, the accept future is dropped and no new
connections are accepted. Existing `handle_client` tasks continue to completion
(tokio task drop behaviour on runtime exit).

## Security & Privacy

- No network socket. No remote shutdown API.
- No authentication required (stdin is local to the process).
- No file writes. No database updates.
- No telemetry, metrics export, or log aggregation.
- No IP addresses, player names, tokens, or credentials in the shutdown
  summary.
- Session IDs in the registry dump are opaque monotonic integers, not IP-based.
- Shutdown state is in-memory only and is dropped when the process exits.

## Hard Boundaries

This feature does not and will not:

- Expose a remote shutdown API.
- Persist shutdown state to disk.
- Add telemetry, metrics export, or log aggregation.
- Forcibly disconnect active sessions (future milestone).
- Execute shutdown scripts or callbacks.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.
