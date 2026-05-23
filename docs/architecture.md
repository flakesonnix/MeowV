# Architecture

## Project Overview

MeowV is a clean-room GTA V-like multiplayer framework prototype. It
implements the networking, resource management, and server runtime
infrastructure that a multiplayer mod would need, but in a fully standalone
environment with no game integration. Inspired by the architecture of FiveM /
alt:V / RAGE MP but independently designed and implemented.

All code is Rust, organised as a multi-crate Cargo workspace. Every feature is
dry-run or report-only — nothing enforces policy, downloads files, or executes
scripts.

## Crate Map

### `crates/protocol`

Shared wire-format types, protocol versioning, and policy evaluation helpers.

| Area | Key Types |
|---|---|
| Messages | `ClientMessage`, `ServerMessage` — line-delimited JSON |
| Versioning | `PROTOCOL_VERSION`, `ProtocolVersionRange` |
| Capabilities | `ProtocolCapability`, `ProtocolCompatibilityProfile` |
| Negotiation | `negotiate_protocol_dry_run`, `ProtocolNegotiationResult` |
| Resource flow | `ResourceAnnouncement`, `AnnouncedResource`, `ResourcePolicyEvaluation`, `ResourceJoinDecision`, `ResourceRequirementLevel` |
| Join gate | `JoinGateDecision`, `JoinGateOutcome`, `build_join_gate_decision` |
| Gate helpers | `capability_gate_report`, `shared_capabilities`, `requires_capability`, `profile_supports_capability` |
| Signature | `check_announcement_signature_stub` (stub, non-enforcing) |
| Entity sync | `EntityState`, `Position` |

Tests: 39

### `crates/resource_manifest`

Standalone resource packaging model. No I/O beyond reading manifest TOML files
and building directory indexes.

| Area | Key Types |
|---|---|
| Manifest | `ResourceManifest` — TOML model |
| Pack index | `build_pack_index`, `PackIndex` — SHA-256, symlink rejection |
| Cache verify | `verify_cache`, `CacheVerificationReport` |
| Registry | `ResourceRegistry`, `ResourceRegistration` — discovery + dep resolution |
| Load plan | `build_load_plan` — Kahn/topo sort, deterministic |
| Runtime SM | `ResourceRuntimeStateMachine`, `ResourceState` (Planned→Validated→Ready→Started→Stopped/Failed) |
| Compatibility | `CompatibilityContext`, `CompatibilityReport`, `check_compatibility` |

Tests: 56

### `crates/server`

Async TCP server with full session lifecycle, admin/debug infrastructure,
config-driven policy, and graceful shutdown. The largest crate.

| Module | Purpose |
|---|---|
| `lib.rs` | `run`, `run_with_listener`, `handle_client`, `accept_loop`, `admin_stdin_loop`, `spawn_tick_loop` |
| `session` | `SessionState`, `SessionStateMachine` — forward-only state machine (Connected → … → ReadyDryRun / Failed) |
| `event_log` | `SessionEventLog`, `SessionEvent` — in-memory per-session audit trail, no timestamps |
| `diagnostics` | `SessionDiagnostics` — read-only snapshot from state machine + event log |
| `session_registry` | `SessionRegistry`, `SessionRegistrySnapshot` — BTreeMap-backed live registry with deterministic ordering |
| `status` | `ServerRuntimeStatus` — config-derived snapshot with live session counts |
| `config` | `ServerConfig` (6 sections), `ConfigError`, `load_from_path`, `load_with_env`, `validate` |
| `admin` | `AdminCommand` (6 variants), `parse_admin_command`, `handle_admin_command`, `handle_admin_command_with_context` |
| `shutdown` | `ShutdownState`, `ShutdownReason`, `ShutdownSummary`, `build_shutdown_summary` |

Tests: 130 (122 unit + 8 integration)

### `crates/client`

Dummy CLI client used for protocol and integration testing. Connects, logs in
with protocol version, sends a chat message, and prints server packets.

Tests: 0

### `crates/game_edition`

Edition / platform detection types. Conservative placeholders — no runtime
detection beyond compile-time `cfg` checks.

| Type | Purpose |
|---|---|
| `GameEdition` | Legacy, Enhanced, Unknown |
| `GamePlatform` | Windows, Linux, Unknown |
| `detect_edition` | Path-based heuristic, not GTA V runtime |

Tests: 4

### `crates/server_browser`

Local JSON server list source and filtering.

| Type | Purpose |
|---|---|
| `ServerEntry` | name, bind, protocol_version, edition |
| `LocalJsonServerListSource` | reads `server_list.json` |
| `filter_servers` | edition + protocol version filtering |

Tests: 4

## Server Module Relationships

```
config ──────────────────────────────────────────┐
                                                 │
       ┌───────────────── session ──── event_log │
       │                      │            │     │
       │                      ▼            ▼     │
       │              diagnostics ◄── session_reg │
       │                      │            │     │
       │                      ▼            ▼     │
  lib.rs ─── admin ─── status ◄── session_reg ───┤
       │                      │                  │
       │                      ▼                  │
       └──────────── shutdown ◄── session_reg ────┘
```

- `lib.rs` owns the runtime and wires all modules together.
- `session` and `event_log` are per-task (local to `handle_client`).
- `session_registry`, `status`, `admin`, `shutdown` are shared across tasks
  via `SharedState` (behind `Arc` + `Mutex`/`RwLock`).
- `config` is read at startup and cloned per task; never mutated at runtime.

## Resource Pipeline (Client-Side, Dry-Run)

```
resource.toml (manifest)
     ↓
pack index (SHA-256, no symlinks)
     ↓
cache verification (SHA-256 compare)
     ↓
resource registry (discovery + dep resolution)
     ↓
load plan (Kahn/topo sort, deterministic)
     ↓
runtime state machine (no-exec: Planned→Validated→Ready→Started→Stopped)
     ↓
compatibility check (edition, protocol version)
```

A future download/repair stage would sit between cache verification and the
resource registry. See `docs/resource-download-design.md` for the design
specification. No download logic is implemented in the current milestone.

## Server Handshake Pipeline

```
TCP connect
     ↓
[SessionStateMachine: Connected]
     ↓  Login { name, protocol_version }
[HelloReceived]
     ↓  protocol version check (exact match enforced)
[VersionChecked] — or — [Failed] + disconnect
     ↓
protocol negotiation dry-run (report-only)
[NegotiationDryRunLogged]
     ↓
capability gate for ResourceAnnouncement (report-only)
[CapabilityGateChecked]
     ↓
send ResourceAnnouncement
[ResourceAnnouncementSent]
     ↓
receive AvailabilityReport
[AvailabilityReportReceived]
     ↓
evaluate resource policy (report-only)
[ResourcePolicyEvaluated]
     ↓
capability gate for JoinGateDryRun (report-only)
[CapabilityGateChecked]
     ↓
send JoinGateDecision (dry-run, no disconnect)
[JoinGateDryRunSent]
     ↓
[ReadyDryRun] — diagnostics printed, session logged
```

Each transition records a `SessionEvent` in the per-task `SessionEventLog` and
updates the live `SessionRegistry`.

## Admin / Debug Pipeline

```
stdin line
     ↓
parse_admin_command (case-insensitive, 6 commands)
     ↓
snapshot registry + build status
     ↓
handle_admin_command_with_context
     ↓
log result at info level
     ↓
quit → ShutdownState::request(AdminQuit) → oneshot signal
     ↓
accept loop stops → build ShutdownSummary → log → return
```

## Current Dry-Run Policies

| Policy | Mode |
|---|---|
| Protocol version matching | **Enforced** — exact match required |
| Protocol negotiation | Dry-run only — no enforcement, logged |
| Join gate | Dry-run only — no disconnect |
| Capability gates | Report-only — logged, no behaviour change |
| Resource announcement signature | Stub — not checked |
| Resource compatibility | Report-only — logged, no enforcement |
| Session state machine | Forward-only, tracks pipeline, no enforcement |
| Admin/debug | Local stdin only, no network |
| Diagnostics registry output | No IP/personal data, deterministic |
| Shutdown summary | In-memory only, no persistence |

## Next Recommended Milestones

- **M3.4**: Local resource cache repair plan — design a cache-repair flow that
  uses only local data. No network. No downloads.
- **M3.5**: Real signature verification design — design or implement
  announcement signature verification for resource authenticity.
- **M3.6**: Download protocol DTOs only — add download message types to the
  protocol crate. No transfer logic.
- **M3.7**: Staging directory model — implement staging directory creation,
  cleanup, and deterministic cache layout. Local-only. No downloads.

## Hard Boundaries

- No GTA V integration. No game runtime dependencies.
- No memory hooks, injection, or anti-cheat bypass.
- No DRM or Rockstar service bypass.
- No downloads, file serving, or script execution.
- No remote admin API, web panel, or network-accessible debug interface.
- No persistence, telemetry, or metrics export.
- No proprietary, leaked, or copied code.
- No Lua, JS, WASM, or any scripting runtime.
- All protocol designs are original, clean-room, and independently documented.
