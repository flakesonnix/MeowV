# Roadmap

## Milestone 0

Standalone prototype:

- Rust workspace
- Nix dev shell
- shared protocol
- server
- dummy client
- login/chat/entity sync

## Milestone 1

Protocol hardening:

- compatibility policy docs
- heartbeat/ping
- better config files
- integration tests

## Milestone 1.8

Protocol compatibility negotiation design:

- dry-run negotiation data structures
- version ranges, capability flags
- intersection/evaluation logic
- server/client dry-run reporting
- docs explaining future activation path

## Milestone 2.6

Server runtime status snapshot:

- `ServerRuntimeStatus` — 13 fields: server identity, protocol policy flags,
  session counts (default 0), resource dir, diagnostics/admin flags
- `from_config(&ServerConfig)` — derives snapshot from config; no timestamps
- `with_session_counts(connected, ready_dry_run, failed)` — returns updated snapshot
- `to_text()` — deterministic `key: value` multi-line output; no client IPs
- `handle_admin_command_with_status(command, Option<&ServerRuntimeStatus>)` — status/sessions/resources commands use snapshot data when provided
- `handle_admin_command` now delegates to `handle_admin_command_with_status(cmd, None)`
- `admin_stdin_loop` accepts `ServerConfig`, builds snapshot at startup, uses status-aware handler
- `docs/server-runtime-status.md`
- 8 status unit tests; 4 admin integration tests

## Milestone 2.5

Local server admin debug commands:

- `AdminCommand` enum (Help, Status, Sessions, Resources, Diagnostics, Quit)
- `AdminCommandParseError` (Empty, UnknownCommand) with Display + Error impls
- `AdminCommandResult` { command, message, should_quit }
- `parse_admin_command` — case-insensitive, whitespace-trimmed, 6 commands
- `handle_admin_command` — placeholder messages; `should_quit=true` for Quit only
- `AdminSection { local_stdin_enabled: bool }` added to `ServerConfig`
- `run_with_listener` refactored into `accept_loop` + `admin_stdin_loop`
- stdin loop gated on `config.admin.local_stdin_enabled`; oneshot channel signals quit
- `example.server.toml` gains `[admin]` section
- 12 admin parser unit tests; 3 AdminSection config tests
- `docs/server-admin-debug-commands.md`

## Milestone 2.4

Structured logging / tracing config:

- `LogLevel` enum (Trace/Debug/Info/Warn/Error), `LogFormat` enum (Text/Json),
  `LoggingSection` in server config
- `[logging]` section in `example.server.toml`
- `init_logging(&LoggingSection)` — branches on format (text/json), applies
  level and show_targets, uses `try_init()` to avoid double-init panics
- `RUST_LOG` env var still takes precedence over config level
- "logging initialized" info line emitted at startup
- main.rs loads config before calling init_logging
- Server already uses `info!/warn!/error!` throughout; no println! present
- 6 new logging config unit tests (all levels parse, invalid level/format
  rejected at parse time, default validates, JSON format parses)

## Milestone 2.3

Server config for dry-run policies:

- `crates/server/src/config.rs` — structured `ServerConfig` with five sections
  (server, protocol, resources, join_gate, diagnostics)
- `ServerConfig::load_from_path()`, `load_with_env()`, `validate()`
- Validation rejects unsafe settings: `exact_version_required=false`,
  `negotiation_dry_run=false`, `enforce_required_resources=true`, path traversal
- `DiagnosticsFormat` and `JoinGateConfigMode` enums with serde deserialization
- Server binary gains `--config <path>` CLI flag
- `MEOWV_SERVER_BIND` and `MEOWV_TICK_RATE` env overrides preserved
- `example.server.toml` updated to new sectioned format
- Diagnostics prints gated on `diagnostics.print_session_diagnostics`
- Diagnostics format switchable (`text` / `json_stub`) via config
- 12 config unit tests

## Milestone 2.2

Session diagnostics / debug dump:

- `SessionDiagnostics` struct collecting current state, history, event log,
  last event message, ready_dry_run flag, failure reason
- `from_parts(&SessionStateMachine, &SessionEventLog) -> Self` — read-only snapshot
- `to_text()` — deterministic human-readable multi-line output
- `to_json_stub()` — manually-formatted JSON, no serde dependency added
- printed to server info log at ReadyDryRun and Failed (version mismatch)
- in-memory only; no persistence, no network exposure, no IP/personal data
- 8 unit tests

## Milestone 2.1

Session event log / audit trail:

- in-memory `SessionEventLog` with `SessionEventKind` variants
- `SessionEvent` carrying sequence, kind, state, and message (no timestamps)
- integrated into `handle_client` alongside every state transition
- records Connected, HelloReceived, VersionChecked, ProtocolNegotiationDryRun,
  CapabilityGateChecked (×2), ResourceAnnouncementSent, AvailabilityReportReceived,
  ResourcePolicyEvaluated, JoinGateDryRunSent, ReadyDryRun, and Failed
- session audit summary logged at session end
- 8 unit tests; no timestamps, no IP/personal data, no persistence

## Milestone 2.0

Server session state machine:

- explicit `SessionState` enum (Connected → … → ReadyDryRun / Failed)
- `SessionStateMachine` with forward-only transitions, terminal Failed state
- `SessionStateError` variants (InvalidTransition, ProtocolMismatch, PolicyBlockedDryRun, …)
- server `handle_client` tracks state through full handshake pipeline
- each transition logged at `info` level; no enforcement changes
- docs explaining state graph, dry-run nature, and future enforcement point

## Milestone 1.9

Protocol capability-gated resource flow:

- capability gate helpers (`profile_supports_capability`, `shared_capabilities`,
  `requires_capability`, `capability_gate_report`)
- server logs capability gate before `ResourceAnnouncement` and `JoinGateDecision`
- client prints local capabilities on connect and `--protocol-negotiation`
- report-only: no enforcement, no disconnects, no behaviour change
- docs explaining gate model and future activation path

## Milestone 1.0

Resource registry:

- multi-resource discovery
- dependency validation
- cycle detection
- deterministic load order

## Milestone 1.1

Runtime boundary:

- no-exec load planning
- runtime separation prep
- deterministic resource planning

## Milestone 1.2

Runtime state machine:

- no-exec lifecycle simulation
- deterministic resource states
- dependency readiness checks

## Milestone 0.5

Game edition layer:

- edition-aware metadata types
- conservative placeholder detection
- clean-room support policy docs
- no runtime GTA V integration


## Milestone 2

Resource/runtime model:

- server resource manifest format
- script/runtime abstraction
- permission model
- hot-reload experiments in standalone environment

## Milestone 3

Transport/runtime refinement:

- snapshot interpolation experiments
- reliability channels
- interest management prototype
- metrics and tracing exports

## Milestone 4

Native boundary evaluation:

- decide whether any low-level bridge is necessary
- if yes, isolate in narrow crate/module
- require legal and architectural review before implementation
