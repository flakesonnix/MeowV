# Session Diagnostics

## Purpose

Session diagnostics provide a compact, structured snapshot of a client
session at the point it reaches a terminal state — either `ReadyDryRun`
(full handshake pipeline complete) or `Failed` (version mismatch). The
snapshot collects all in-memory session data in one place and formats it
for immediate log output.

This replaces the need to scan multiple log lines to reconstruct session
state. The diagnostics snapshot is deterministic: given the same session
machine and event log, it always produces the same output.

## Scope and Privacy Constraints

- **In-memory only.** The diagnostics object lives on the stack inside
  `handle_client` and is dropped after printing. Nothing is persisted.
- **No IP addresses or personal data.** Event messages may contain
  client-provided names (from the `Login` packet), but no network
  addresses, tokens, or other personal data.
- **No network exposure.** Diagnostics are never sent to the client or
  any external system.
- **No telemetry.** Diagnostics are printed to the local structured log
  (`tracing info!`) only. There is no telemetry sink, metrics pipeline, or
  external reporting.
- **No file writing.** No log files are opened or written by the
  diagnostics layer.
- **No timestamps in the snapshot.** Individual structured log lines carry
  real wall-clock timestamps via `tracing`. The `SessionDiagnostics` struct
  itself stores no time values, which keeps unit tests deterministic.

## `SessionDiagnostics` Fields

| Field | Type | Meaning |
|---|---|---|
| `current_state` | `SessionState` | State of the session machine at snapshot time. |
| `state_history` | `Vec<SessionState>` | States the machine has left, in order. Does not include `current_state`. |
| `event_count` | `usize` | Total number of events recorded in the session log. |
| `events` | `Vec<SessionEvent>` | Full ordered copy of the session event log. |
| `last_event_message` | `Option<String>` | Message string of the most recent event, or `None` if log is empty. |
| `ready_dry_run` | `bool` | `true` iff `current_state == ReadyDryRun`. |
| `failure_reason` | `Option<String>` | Failure message from the state machine, or `None` if session did not fail. |

## API

```rust
pub struct SessionDiagnostics { /* fields above */ }

impl SessionDiagnostics {
    /// Collect snapshot from the live in-memory session machine and event log.
    /// Does not mutate either source.
    pub fn from_parts(machine: &SessionStateMachine, log: &SessionEventLog) -> Self;

    /// Format as a human-readable multi-line text block.
    /// Output is deterministic.
    pub fn to_text(&self) -> String;

    /// Format as a manually-constructed JSON object.
    /// Output is deterministic. Not derived from serde — no extra dependencies.
    pub fn to_json_stub(&self) -> String;
}
```

`from_parts` clones the current state, history slice, and events slice from
the supplied references. Neither argument is mutated.

## Text Output Format

`to_text()` produces a multi-line string with one key per line:

```
state: ReadyDryRun
ready_dry_run: true
state_history: Connected HelloReceived VersionChecked NegotiationDryRunLogged ResourceAnnouncementSent AvailabilityReportReceived ResourcePolicyEvaluated JoinGateDryRunSent
event_count: 11
  [1] Connected @ Connected: client connected
  [2] HelloReceived @ HelloReceived: login from alice
  [3] VersionChecked @ VersionChecked: protocol version 1 matched
  [4] ProtocolNegotiationDryRun @ NegotiationDryRunLogged: negotiation status: ExactMatch
  [5] CapabilityGateChecked @ NegotiationDryRunLogged: ResourceAnnouncement gate: supported=false
  [6] ResourceAnnouncementSent @ ResourceAnnouncementSent: resource announcement sent to client
  [7] AvailabilityReportReceived @ AvailabilityReportReceived: resource availability report received from client
  [8] ResourcePolicyEvaluated @ ResourcePolicyEvaluated: policy decision: Allowed
  [9] CapabilityGateChecked @ ResourcePolicyEvaluated: JoinGateDryRun gate: supported=false
  [10] JoinGateDryRunSent @ JoinGateDryRunSent: join gate dry-run decision sent to client
  [11] ReadyDryRun @ ReadyDryRun: handshake pipeline complete (dry-run)
last_event: handshake pipeline complete (dry-run)
```

For a failed session the output includes a `failure_reason` line:

```
state: Failed
ready_dry_run: false
failure_reason: protocol mismatch: client=99 server=1
state_history: Connected HelloReceived
event_count: 3
  [1] Connected @ Connected: client connected
  [2] HelloReceived @ HelloReceived: login from alice
  [3] Failed @ Failed: protocol mismatch: client=99 server=1
last_event: protocol mismatch: client=99 server=1
```

If the state history is empty (new session, no transitions yet),
`state_history: (none)` is printed.

## JSON Stub Format

`to_json_stub()` produces a single-line JSON object:

```json
{"current_state":"ReadyDryRun","ready_dry_run":true,"failure_reason":null,"state_history":["Connected","HelloReceived",...],"event_count":11,"events":[{"seq":1,"kind":"Connected","state":"Connected","message":"client connected"},...],"last_event_message":"handshake pipeline complete (dry-run)"}
```

The JSON stub is manually formatted — no serde derives are used. The format
is stable for human inspection and future tooling but is not guaranteed to
match any external schema.

## Integration in `handle_client`

Diagnostics are built and printed at two points:

1. **ReadyDryRun reached** — after `mark_ready_dry_run()` succeeds:
   ```
   session diagnostics:
   state: ReadyDryRun
   ...
   ```

2. **Failed on version mismatch** — after the Failed event is recorded,
   before the `Disconnect` message is sent:
   ```
   session diagnostics (failed):
   state: Failed
   failure_reason: protocol mismatch: client=99 server=1
   ...
   ```

Both prints are at `tracing info!` level under the `client_id` field.
No other code paths trigger diagnostics in this milestone.

## Relation to Session State Machine and Event Log

| Component | Role |
|---|---|
| `SessionStateMachine` | Enforces transition validity; holds current state, history, failure reason. |
| `SessionEventLog` | Append-only event record; holds typed events with sequence numbers and messages. |
| `SessionDiagnostics` | Read-only snapshot from both; formats for output. Never feeds back into either. |

The diagnostics layer is strictly one-directional. It reads from the other
two components; it does not write to them.

## Determinism

`to_text()` and `to_json_stub()` are both deterministic:

- Event order matches `SessionEventLog` insertion order (sequence 1…N).
- State history order matches `SessionStateMachine` transition order.
- No maps or sets are iterated; no hash-dependent ordering.
- No timestamps, random values, or environment reads.

Calling either method twice on the same `SessionDiagnostics` instance
produces identical output.

## Future Extension Points

- Persist a bounded ring buffer of recent session diagnostics for
  post-mortem inspection via an admin interface.
- Expose via a local diagnostics socket (never over the game protocol
  channel).
- Drive integration test assertions by comparing the snapshot directly
  rather than parsing log output.
- Add wall-clock timestamps as an optional field without breaking
  deterministic unit tests.
- Emit `to_json_stub()` output instead of text when structured JSON log
  output is required.

## Hard Boundaries

This feature does not and will not:

- Send diagnostics to the client over the game protocol.
- Persist diagnostics to disk or any database.
- Include IP addresses or personal data.
- Add timestamps to the `SessionDiagnostics` struct fields.
- Add downloads, file serving, or script execution.
- Integrate with GTA V or any proprietary system.
- Enable real protocol negotiation or enforce the join gate.
- Use leaked, proprietary, or copied implementation details.

All diagnostic output is local to the server process, produced from
clean-room in-memory state, and disappears when the process exits.
