# Server Configuration

## Purpose

The MeowV server reads a TOML configuration file at startup. The config
file controls bind address, server name, tick rate, protocol policy flags,
local resource paths, join gate mode, and diagnostics output. Without a
config file the server starts with safe defaults and operates identically
to earlier milestones.

## Loading

Pass the config file path via the `--config` flag:

```
cargo run --bin server -- --config example.server.toml
```

If `--config` is not given, the server checks the `MEOWV_CONFIG`
environment variable. If neither is set, all defaults apply.

After the file is loaded, two environment overrides are applied:

| Variable | Effect |
|---|---|
| `MEOWV_SERVER_BIND` | Overrides `server.bind_addr` |
| `MEOWV_TICK_RATE` | Overrides `server.tick_rate` |

Environment overrides are applied after file loading, so they take
precedence over the file.

## Sections

### `[server]`

```toml
[server]
bind_addr = "127.0.0.1:7000"
name = "MeowV Local Dev Server"
tick_rate = 10
motd = "welcome to meowv milestone 0"
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `bind_addr` | `String` | `127.0.0.1:7000` | TCP address and port to listen on. Must parse as a valid `SocketAddr`. |
| `name` | `String` | `MeowV Local Dev Server` | Human-readable server name. Not yet used in the wire protocol. |
| `tick_rate` | `u64` | `10` | Entity snapshot ticks per second. |
| `motd` | `String` | `welcome to meowv milestone 0` | Message of the day sent to clients on connect. |

### `[protocol]`

```toml
[protocol]
exact_version_required = true
negotiation_dry_run = true
capability_gates_report_only = true
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `exact_version_required` | `bool` | `true` | Require clients to match the server's exact protocol version. **Must remain `true`.** |
| `negotiation_dry_run` | `bool` | `true` | Protocol negotiation is computed and logged but not enforced. **Must remain `true`.** |
| `capability_gates_report_only` | `bool` | `true` | Capability gate results are logged but do not block the session. |

Both `exact_version_required` and `negotiation_dry_run` are validated on
load. Setting either to `false` causes the server to refuse to start with
a clear error message. Enforcement is not yet implemented.

### `[resources]`

```toml
[resources]
announcement_resource_dir = "examples/resources/chat"
cache_dir = "examples/cache/chat-valid"
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `announcement_resource_dir` | `String` | `examples/resources/chat` | Directory of the resource pack the server announces to clients. Relative paths are resolved from the workspace root. |
| `cache_dir` | `String` | `examples/cache/chat-valid` | Local cache directory (used by the client in verification flows; not read by the server directly in this milestone). |

**Path safety:** Both paths are validated. Paths containing `..` are
rejected to prevent directory traversal. Only relative paths pointing
inside the workspace or absolute paths are accepted.

**Path resolution:** Relative paths in `announcement_resource_dir` are
resolved from the workspace root at runtime using the server binary's
compile-time `CARGO_MANIFEST_DIR` anchor. Absolute paths are used as-is.

### `[join_gate]`

```toml
[join_gate]
mode = "dry_run"
enforce_required_resources = false
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `mode` | `"dry_run"` | `dry_run` | Join gate evaluation mode. Only `dry_run` is supported. |
| `enforce_required_resources` | `bool` | `false` | Whether to block clients missing required resources. **Must remain `false`.** |

Setting `enforce_required_resources = true` causes the server to refuse
to start. Join gate enforcement is not yet implemented.

### `[diagnostics]`

```toml
[diagnostics]
print_session_diagnostics = true
print_event_log = false
format = "text"
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `print_session_diagnostics` | `bool` | `true` | Print a structured session diagnostics snapshot at ReadyDryRun and Failed. |
| `print_event_log` | `bool` | `false` | Print the full event log inline with the diagnostics (verbose). |
| `format` | `"text"` \| `"json_stub"` | `text` | Output format for diagnostics. `text` is human-readable multi-line; `json_stub` is a single-line manually-formatted JSON object. |

Unknown format values are rejected at parse time.

## Validation

`ServerConfig::validate()` is called after every file load. Validation
rules enforced:

1. `protocol.exact_version_required` must be `true`.
2. `protocol.negotiation_dry_run` must be `true`.
3. `join_gate.enforce_required_resources` must be `false`.
4. `resources.announcement_resource_dir` must not contain `..`.
5. `resources.cache_dir` must not contain `..`.
6. `server.bind_addr` must parse as a valid `SocketAddr`.

A validation failure is a hard error — the server exits before binding.

## Partial Config

All sections and all fields are optional. Missing fields fall back to
their defaults. A minimal valid config file:

```toml
[server]
bind_addr = "0.0.0.0:7000"
motd = "my server"
```

All other fields use defaults.

## Example Config

See `example.server.toml` in the workspace root for a fully-annotated
example covering all sections.

## Lifecycle Summary

`ServerConfig::to_lifecycle_summary_text()` produces a deterministic multi-line
text dump of the server's lifecycle configuration at startup. It is logged
as a single `info` event immediately after the bind line:

```
INFO server lifecycle config:
server_name: MeowV Local Dev Server
bind_addr: 127.0.0.1:7000
protocol_version: 17
exact_version_required: true
negotiation_dry_run: true
capability_gates_report_only: true
resource_announcement_dir: examples/resources/chat
join_gate_mode: dry_run
join_gate_enforcement: disabled
diagnostics_print: true
diagnostics_format: text
admin_stdin: disabled
log_level: info
log_format: text
```

The summary covers all active config sections and policy flags. No IP
addresses, personal data, or timestamps appear in the output.

## Privacy and Security

- Config values are logged at startup (`info` level: bind address, tick
  rate, server name, full lifecycle summary).
- Resource paths are relative to the workspace root and must not contain
  `..`.
- No IP addresses from connected clients are stored in the config.
- No credentials, tokens, or secrets are stored in the config.
- No network access is performed as part of config loading.
- Config file reading uses standard `std::fs::read_to_string` — no
  symlink following for the file itself (OS-dependent; the file path is
  provided by the operator).

## Hard Boundaries

This feature does not and will not:

- Relax exact protocol version matching via config.
- Enable join gate enforcement via config.
- Add downloads, file serving, or script execution.
- Add remote config fetching or hot-reload.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All config loading is local to the server process, deterministic, and
produces no side-effects beyond reading one TOML file and setting
in-memory fields.
