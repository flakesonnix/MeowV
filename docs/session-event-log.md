# Session Event Log

## Purpose

The session event log is an in-memory structured audit trail that runs
alongside the session state machine in `handle_client`. Each significant
protocol event is recorded as a `SessionEvent` with a monotonic sequence
number, the event kind, the current session state, and a human-readable
message. The log is local to the client handler task and never shared.

It serves two goals:

1. **Observability** — every step of the handshake pipeline is captured in
   a typed, queryable structure rather than only in log lines.
2. **Test fixture** — unit tests can drive the log directly against
   `SessionEventLog` without a running server.

## Event Kinds (`SessionEventKind`)

| Variant | When recorded |
|---|---|
| `Connected` | TCP connection accepted; first event in every session. |
| `HelloReceived` | `Login` packet parsed successfully. |
| `VersionChecked` | Client protocol version matched the server. |
| `ProtocolNegotiationDryRun` | Dry-run negotiation result computed and logged. |
| `CapabilityGateChecked` | A capability gate report evaluated (recorded twice per session: before `ResourceAnnouncement` and before `JoinGateDecision`). |
| `ResourceAnnouncementSent` | Server sent the resource list to the client. |
| `AvailabilityReportReceived` | Client sent its resource availability report. |
| `ResourcePolicyEvaluated` | Resource join policy evaluated against the report. |
| `JoinGateDryRunSent` | Dry-run join gate decision sent to the client. |
| `ReadyDryRun` | Full handshake pipeline complete (dry-run). |
| `Failed` | Version mismatch detected; session terminated. |

## `SessionEvent` Fields

| Field | Type | Meaning |
|---|---|---|
| `sequence` | `u64` | Monotonic counter starting at 1. Unique per log. |
| `kind` | `SessionEventKind` | What happened. |
| `state` | `SessionState` | Session state at the time of recording. |
| `message` | `String` | Human-readable detail (version numbers, policy decision, capability name, etc.). |

No timestamps are stored. Timestamps are intentionally omitted:
deterministic unit tests do not need them, and structured log lines
(emitted via `tracing`) carry real wall-clock times.

No IP addresses or personal data are stored in the event log.

## `SessionEventLog` API

```rust
pub struct SessionEventLog { /* ... */ }

impl SessionEventLog {
    pub fn new() -> Self;
    pub fn record(&mut self, kind: SessionEventKind, state: SessionState, message: impl Into<String>);
    pub fn events(&self) -> &[SessionEvent];
    pub fn last(&self) -> Option<&SessionEvent>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

`record` appends a new event and increments the sequence counter.
`events` returns the full ordered slice.

## Integration in `handle_client`

The log is created at the start of `handle_client`:

```rust
let mut event_log = SessionEventLog::new();
event_log.record(SessionEventKind::Connected, SessionState::Connected, "client connected");
```

From that point, each successful state transition or significant gate check
appends one event. The log is never moved into the spawned writer task — it
stays in the handler's stack frame for its full lifetime.

At session end, a summary log line is emitted:

```
session audit log complete  event_count=N  final_state=ReadyDryRun
```

The full sequence of events recorded in a successful session:

1. `Connected` — "client connected"
2. `HelloReceived` — "login from {name}"
3. `VersionChecked` — "protocol version {v} matched"
4. `ProtocolNegotiationDryRun` — "negotiation status: ExactMatch"
5. `CapabilityGateChecked` — "ResourceAnnouncement gate: supported=false"
6. `ResourceAnnouncementSent` — "resource announcement sent to client"
7. `AvailabilityReportReceived` — "resource availability report received from client"
8. `ResourcePolicyEvaluated` — "policy decision: Allowed"
9. `CapabilityGateChecked` — "JoinGateDryRun gate: supported=false"
10. `JoinGateDryRunSent` — "join gate dry-run decision sent to client"
11. `ReadyDryRun` — "handshake pipeline complete (dry-run)"

On version mismatch the sequence is:

1. `Connected`
2. `HelloReceived`
3. `Failed` — "protocol mismatch: client={v} server={PROTOCOL_VERSION}"

## Relation to Session State Machine

The event log and state machine are parallel but independent:

- The state machine enforces transition validity and returns errors on bad
  transitions.
- The event log appends unconditionally whenever the caller decides an event
  is worth recording.

They share `SessionState` values but neither holds a reference to the other.

## Current Behaviour

The log is in-memory only. It is not persisted, not sent over the wire, and
not exposed outside `handle_client`. Its primary value in this milestone is
testability and structured tracing alongside the state machine.

## Future Extension Points

- Persist to a ring buffer or bounded log per client for post-mortem
  inspection.
- Expose via an admin or diagnostics endpoint.
- Drive integration test assertions by inspecting the ordered event sequence
  rather than parsing log output.

## Hard Boundaries

This feature does not and will not:

- Store IP addresses or personal data in event messages.
- Send event log contents to clients.
- Persist events to disk or an external store.
- Add timestamps to individual events (use structured log output for that).
- Add downloads, file serving, script execution, or GTA V integration.
- Use leaked, proprietary, or copied implementation details.
