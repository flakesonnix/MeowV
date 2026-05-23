# Server Logging

## Purpose

The MeowV server emits structured log output for every significant lifecycle
event: server startup, client connect/disconnect, each session state
transition, protocol negotiation, capability gates, resource policy
evaluation, join gate dry-run, and session diagnostics. All log output is
local to the server process. There is no telemetry, no remote reporting, and
no file rotation.

## Configuration

Logging is configured through the `[logging]` section of the server TOML
config (see `example.server.toml`):

```toml
[logging]
level = "info"
format = "text"
show_targets = false
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `level` | log level string | `info` | Minimum log level to emit. |
| `format` | format string | `text` | Output format: `text` or `json`. |
| `show_targets` | `bool` | `false` | Whether to include the Rust module path in each log line. |

## Log Levels

Supported values for `level`, from most to least verbose:

| Value | Meaning |
|---|---|
| `trace` | Very verbose; includes internal loop details. |
| `debug` | Useful for development; shows per-packet details. |
| `info` | Recommended default; shows all session lifecycle events. |
| `warn` | Warnings and non-fatal errors only. |
| `error` | Fatal errors only. |

Unknown level strings are rejected at config parse time.

The `RUST_LOG` environment variable overrides the configured level at runtime.
Set `RUST_LOG=debug` to increase verbosity without changing the config file.

## Log Formats

| Value | Description |
|---|---|
| `text` | Human-readable multi-line output via `tracing-subscriber::fmt`. |
| `json` | Machine-readable newline-delimited JSON. Each log line is one JSON object. |

Unknown format strings are rejected at config parse time.

The `json` format uses the `tracing-subscriber` built-in JSON layer. It adds
no external dependencies beyond what is already in the workspace.

## Startup Log

When `init_logging` completes, the server emits:

```
INFO logging initialized  level=info  format=Text  show_targets=false
```

This appears as the first log line and confirms the active logging
configuration.

## What Is Logged

The server emits structured `info`-level logs at every key point:

| Event | Key fields |
|---|---|
| Server listening | `bind`, `tick_rate`, `name` |
| Client connected | `addr` |
| Login received | `client_id`, `state` |
| Version check passed | `client_id`, `state` |
| Version mismatch | `client_id`, `state` |
| Protocol negotiation dry-run | `client_version`, `server_version`, `negotiation_status`, `shared_capability_count` |
| Capability gate check | `client_id`, `capability`, `supported`, `reason` |
| Resource announcement sent | `client_id`, `state` |
| Availability report received | `client_id`, `state` |
| Resource policy evaluated | `client_id`, `decision`, missing/invalid counts |
| Join gate dry-run evaluated | `client_id`, `outcome`, `reason` |
| Session ready (dry-run) | `client_id`, `state` |
| Session diagnostics | `client_id`, formatted snapshot |
| Session audit log complete | `client_id`, `event_count`, `final_state` |

`warn!` is used for unexpected state machine transitions and missing data.
`error!` is used for broadcast channel failures.

## Privacy

- Log lines include `client_id` (a UUID generated per connection), not IP
  addresses. IP address appears once at connection time from `addr` (the
  peer socket address) and is not propagated further into session logs.
- Resource names, protocol versions, and policy decisions are logged.
- Client-provided player names appear in session event messages.
- No passwords, tokens, or credentials are logged.
- No persistent log files are written. Output goes to stdout only.

## Relation to Session Diagnostics and Event Log

| Component | Output |
|---|---|
| Structured logs (`tracing`) | Per-event `info!`/`warn!`/`error!` lines emitted as events happen. Ephemeral. |
| Session event log (`SessionEventLog`) | In-memory ordered record of session events. Never written to stdout directly. |
| Session diagnostics (`SessionDiagnostics`) | Snapshot collected at ReadyDryRun / Failed; formatted and emitted via a single `info!` log line. Controlled by `diagnostics.print_session_diagnostics`. |

The structured logs and the session diagnostics complement each other: logs
provide a chronological stream; diagnostics provide a compact post-handshake
summary.

## Future JSON Logging

`format = "json"` is supported now and produces newline-delimited JSON via
`tracing-subscriber`'s built-in JSON layer. This format is suitable for
ingestion by log aggregators (e.g., Loki, Elasticsearch) in a future local
deployment scenario.

No external telemetry service is configured and none will be added.

## Hard Boundaries

This feature does not and will not:

- Write log output to files.
- Add log rotation or size limits.
- Send logs to external services or telemetry endpoints.
- Include IP addresses beyond the initial connection log line.
- Log passwords, tokens, or credentials.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All log output is local, ephemeral, and limited to stdout.
