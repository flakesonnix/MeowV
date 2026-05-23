# Live Session Enforcement (Milestone 4.2)

## Overview

Session enforcement is wired into `handle_client` — the server's per-client
handshake handler. Under `SessionEnforcementPolicy::Strict`, invalid handshake
and session flows now disconnect the client with a structured reason. Under
`SessionEnforcementPolicy::ReportOnly` (the default), all existing behavior
is preserved; enforcement context is added to diagnostics for observability.

## Policy Modes

| Mode | Behavior |
|------|----------|
| `ReportOnly` | No enforcement. Existing disconnect paths (non-Login first message, version mismatch) continue to work. Enforcement decisions are logged in diagnostics when the session fails. |
| `Strict` | Disconnect on any non-Allow enforcement decision. Successful handshakes reach `ReadyDryRun` as before. |

## Enforcement Points

### Hard-Failure Points (already disconnects)

These paths already disconnect the client regardless of policy. Under M4.2,
their diagnostics output now includes `with_enforcement()` context:

- **Non-Login first message** — `Disconnect(InvalidHandshake)`
- **`on_hello_received()` error** — `Disconnect(InvalidHandshake)`
- **Version mismatch** — `Disconnect(ProtocolMismatch)`

### Soft-Failure Points (new enforcement under Strict)

These paths previously logged a warning and continued. Under `Strict`, they
now fail the session and disconnect:

| Transition | Disconnect Reason |
|---|---|
| `on_negotiation_logged()` failure | `InvalidHandshake` |
| `on_resource_announcement_sent()` failure | `InvalidHandshake` |
| `on_availability_report_received()` failure | `InvalidHandshake` |
| `on_policy_evaluated()` non-blocked error | `InvalidHandshake` |
| `on_join_gate_sent()` failure | `InvalidHandshake` |
| `mark_ready_dry_run()` failure | `InvalidHandshake` |

## Architecture

### Pre-Writer vs Post-Writer

Enforcement points are partitioned by whether the per-client writer task
has been spawned:

- **Pre-writer** (negotiation, resource announcement): `send_direct()` writes
  the `Disconnect` message immediately, then the handler returns `Ok(())`.
- **Post-writer** (availability report, policy, join gate, ready): the
  `Disconnect` is sent through `client_tx` (mpsc channel), the handler
  breaks from the main loop, and normal cleanup (writer abort, registry
  removal via `SessionGuard`) runs.

### Enforcement Flow (Strict)

1. Transition fails → `warn!` logged
2. `session.fail(reason)` — marks session `Failed` with reason string
3. `handle_enforcement()` called:
   - Evaluates enforcement via `evaluate_enforcement()`
   - Records `SessionEventKind::Failed` in event log
   - Updates registry to `Failed` state
   - Prints diagnostics with enforcement context (if enabled)
   - Sends `Disconnect` with appropriate reason + message
   - Returns `Ok(true)` → caller disconnects or breaks

### Enforcement Flow (ReportOnly)

1. Transition fails → `warn!` logged
2. Handler continues (existing behavior preserved)
3. If session is `Failed` and diagnostics enabled, `handle_enforcement()`
   prints diagnostic with enforcement context

## Registry Cleanup

All enforcement paths preserve `SessionGuard` RAII cleanup. The session
is set to `Failed` in the registry before the handler returns; on drop,
`SessionGuard` removes the entry entirely.

## Testing

- 5 new integration tests in `tests/session_enforcement.rs`
- Covers: ReportOnly success, Strict success, Strict version mismatch,
  Strict invalid first message, registry cleanup after enforcement
- All 351 workspace tests pass

## Hard Boundaries

- No protocol wire-format changes
- No client capabilities in `Login`
- No heartbeat/ping
- No resource download/cache changes
- No resource execution
- No silent fallback from Strict to ReportOnly
- No broad config redesign
- No new dependencies
