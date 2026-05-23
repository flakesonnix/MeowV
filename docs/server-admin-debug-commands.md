# Server Admin Debug Commands

## Purpose

The MeowV server includes a local-only admin debug command interface. When
enabled, the server reads lines from stdin and executes simple diagnostic
commands. There is no network exposure and no authentication is required
because access is limited to the local process's standard input.

## Configuration

Enable the admin loop in the `[admin]` section of the server TOML config:

```toml
[admin]
local_stdin_enabled = false
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `local_stdin_enabled` | `bool` | `false` | Enable the stdin admin command loop. |

The loop is disabled by default. When disabled, the server never reads from
stdin.

## Commands

Commands are case-insensitive. Leading and trailing whitespace is trimmed.
Blank lines are silently ignored.

| Command | Description |
|---|---|---|
| `help` | Print the list of available commands. |
| `status` | Report server running state and active policy mode (live). |
| `sessions` | Show live per-session details from the session registry. Includes session ID, state, event count, protocol version, ready_dry_run and failed flags. Falls back to aggregate counts when registry is unavailable. |
| `resources` | Show configured announcement resource directory. |
| `diagnostics` | Dump live session diagnostics from `SessionRegistry` snapshot. Shows session IDs, state, event count, ready_dry_run, failed, protocol version. No IP addresses or personal data. |
| `quit` | Request a clean server shutdown. |

## Output

Each command produces a single-line result logged at `info` level via
`tracing`. Example:

```
INFO admin  message=commands: help, status, sessions, resources, diagnostics, quit
INFO admin  message=server is running (dry-run mode, all policies report-only)
INFO admin  message=server shutdown requested via admin command
```

The `sessions` command shows per-session details when the registry is
available:

```
INFO admin  message=sessions: 2  ready_dry_run: 1  failed: 0
   session-1: state=ReadyDryRun  events=11  ready_dry_run=true  failed=false  protocol=v1
   session-2: state=Connected  events=3  ready_dry_run=false  failed=false  protocol=v1
```

Unknown commands are logged as errors:

```
INFO admin command error  error=unknown command: reboot
```

## Shutdown Behaviour

When `quit` is issued, the server stops accepting new connections and returns
from `run_with_listener`. In-flight client sessions that were already spawned
continue to completion (tokio task drop behaviour). There is no graceful drain.

When `local_stdin_enabled = true` and stdin is closed (e.g., piped input
reaches EOF), the admin loop exits silently and the server stops. This is
consistent with standard Unix daemon behaviour when stdin is a pipe.

## Security

- No network socket. No remote access.
- No authentication required because stdin is local to the process.
- No passwords, tokens, or credentials are accepted or emitted.
- Commands only return pre-formatted string messages; no user-supplied data
  is echoed back.

## Implementation

| Symbol | Location |
|---|---|
| `AdminCommand` | `crates/server/src/admin.rs` |
| `AdminCommandParseError` | `crates/server/src/admin.rs` |
| `AdminCommandResult` | `crates/server/src/admin.rs` |
| `parse_admin_command` | `crates/server/src/admin.rs` |
| `handle_admin_command` | `crates/server/src/admin.rs` |
| `handle_admin_command_with_status` | `crates/server/src/admin.rs` |
| `handle_admin_command_with_context` | `crates/server/src/admin.rs` |
| `SessionRegistryEntry` | `crates/server/src/session_registry.rs` |
| `SessionRegistrySnapshot` | `crates/server/src/session_registry.rs` |
| `AdminSection` | `crates/server/src/config.rs` |
| `admin_stdin_loop` | `crates/server/src/lib.rs` (private) |
| `accept_loop` | `crates/server/src/lib.rs` (private) |

## Live vs Placeholder Status

`status`, `sessions`, and `diagnostics` return live data from the server runtime
status snapshot and the session registry. `sessions` prefers registry data
(per-session details) and falls back to aggregate counts from the status
snapshot. `resources` returns the configured announcement directory from the
server config. No command currently returns placeholder messages.

## Hard Boundaries

This feature does not and will not:

- Expose a network socket for remote admin access.
- Require or check authentication credentials.
- Execute arbitrary commands or scripts.
- Accept or log passwords, tokens, or credentials.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.
