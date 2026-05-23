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
|---|---|
| `help` | Print the list of available commands. |
| `status` | Report server running state and active policy mode. |
| `sessions` | List current session data (placeholder; live data not yet wired). |
| `resources` | List announced resource data (placeholder; live data not yet wired). |
| `diagnostics` | Dump session diagnostics (placeholder; live data not yet wired). |
| `quit` | Request a clean server shutdown. |

## Output

Each command produces a single-line result logged at `info` level via
`tracing`. Example:

```
INFO admin  message=commands: help, status, sessions, resources, diagnostics, quit
INFO admin  message=server is running (dry-run mode, all policies report-only)
INFO admin  message=server shutdown requested via admin command
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
| `AdminSection` | `crates/server/src/config.rs` |
| `admin_stdin_loop` | `crates/server/src/lib.rs` (private) |
| `accept_loop` | `crates/server/src/lib.rs` (private) |

## Placeholder Status

`sessions`, `resources`, and `diagnostics` return placeholder messages. Live
session data is not yet wired to the admin command handler. A future milestone
will replace these with real data drawn from the shared server state.

## Hard Boundaries

This feature does not and will not:

- Expose a network socket for remote admin access.
- Require or check authentication credentials.
- Execute arbitrary commands or scripts.
- Accept or log passwords, tokens, or credentials.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.
