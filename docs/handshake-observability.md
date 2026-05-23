# Handshake Observability Guarantees

After Milestone 2.8, every observable subsystem is guaranteed to agree during a
server handshake. The integration tests in
`crates/server/tests/handshake_observability.rs` prove this by inspecting the
live session registry directly through the test-only
`run_with_listener_and_state()` accessor.

## What Is Observable

During a client handshake, these subsystems are kept consistent:

| Subsystem | What it proves |
|-----------|---------------|
| **Session state machine** | Forward-only transitions from Connected → ReadyDryRun or Failed |
| **Event log** | Every transition recorded as a `SessionEvent` with deterministic sequence |
| **Session registry** | Live count, per-session state, ready_dry_run/failed flags |
| **Wire protocol** | All expected server messages arrive in correct order |
| **SessionGuard RAII** | Session removed from registry on handler exit (success or failure) |

## Integration Test Guarantees

Each integration test asserts consistency across all layers:

- **`full_handshake_creates_session_and_reaches_ready_dry_run`**
  - Registry starts empty
  - Session exists after Login → Welcome
  - Session arrives at ReadyDryRun (event_count = 11)
  - Registry shows 1 connected, 1 ready_dry_run, 0 failed
  - Session removed from registry after disconnect

- **`version_mismatch_disconnects_and_cleans_up_session`**
  - Registry starts empty
  - Client receives `Disconnect { ProtocolMismatch }`
  - SessionGuard removes session from registry

- **`invalid_handshake_first_message_not_login`**
  - Client receives `Disconnect { InvalidHandshake }`
  - Session cleaned up

- **`registry_session_id_is_deterministic`**
  - First session always gets deterministic ID "session-1"
  - protocol_version is set correctly on entry

- **`session_created_on_connect_before_login`**
  - Session exists in Connected state before any message is sent
  - protocol_version is None before version check
  - Session removed on disconnect

- **`session_cleaned_up_on_early_disconnect`**
  - Session created on connect even when client sends nothing
  - SessionGuard removes it on connection drop
  - All counts (connected/failed/ready_dry_run) return to zero

- **`runtime_status_reflects_live_session_counts`**
  - `ServerRuntimeStatus::with_session_counts` matches registry snapshot
  - Status text contains exact connected/ready_dry_run/failed counts
  - Second client increases connected_sessions without altering ready_dry_run
  - Status text is consistent across multiple connections

## Runtime Status Consistency

When built from config and the live registry snapshot,
`ServerRuntimeStatus::with_session_counts` reflects the exact same
connected/ready_dry_run/failed counts as the registry. The integration tests
assert this at multiple points during the handshake.

## Event Count Contract

A full successful handshake records exactly 11 events in order:

| # | Event Kind | State |
|---|-----------|-------|
| 1 | `Connected` | `Connected` |
| 2 | `HelloReceived` | `HelloReceived` |
| 3 | `VersionChecked` | `VersionChecked` |
| 4 | `ProtocolNegotiationDryRun` | `NegotiationDryRunLogged` |
| 5 | `CapabilityGateChecked` | `NegotiationDryRunLogged` |
| 6 | `ResourceAnnouncementSent` | `ResourceAnnouncementSent` |
| 7 | `AvailabilityReportReceived` | `AvailabilityReportReceived` |
| 8 | `ResourcePolicyEvaluated` | `ResourcePolicyEvaluated` |
| 9 | `CapabilityGateChecked` | `ResourcePolicyEvaluated` |
| 10 | `JoinGateDryRunSent` | `JoinGateDryRunSent` |
| 11 | `ReadyDryRun` | `ReadyDryRun` |

## Hard Boundaries

- No IP addresses, timestamps, or personal data in any observable output
- No network exposure of diagnostics or registry state
- No behavior changes from test-only code paths
- `SharedState` fields are pub but only used in integration tests
- `run_with_listener_and_state` is the only test-only production addition
