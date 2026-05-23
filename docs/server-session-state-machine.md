# Server Session State Machine

## Purpose

The server session state machine replaces implicit "message happened, then
next message happened" logic with an explicit, auditable state graph. Each
client connection progresses through named states that correspond to concrete
protocol events. This makes the session lifecycle observable in logs, testable
in isolation, and ready for enforcement in a future milestone.

## State Graph

```
Connected
  ↓ Login packet received
HelloReceived
  ↓ Protocol version matched
VersionChecked
  ↓ Dry-run negotiation computed and logged
NegotiationDryRunLogged
  ↓ ResourceAnnouncement sent to client
ResourceAnnouncementSent
  ↓ ResourceAvailabilityReport received from client
AvailabilityReportReceived
  ↓ Resource policy evaluated
ResourcePolicyEvaluated
  ↓ JoinGateDecision sent to client
JoinGateDryRunSent
  ↓ Handshake pipeline complete (dry-run)
ReadyDryRun

Any state → Failed (terminal, on error or explicit fail())
```

## State Descriptions

| State | Meaning |
|---|---|
| `Connected` | TCP connection established; no messages exchanged yet. |
| `HelloReceived` | A valid `Login` message was parsed. |
| `VersionChecked` | Client protocol version matched the server's exact version. |
| `NegotiationDryRunLogged` | Dry-run protocol negotiation result was computed and logged. |
| `ResourceAnnouncementSent` | Server sent the resource list to the client. |
| `AvailabilityReportReceived` | Client sent its resource availability report. |
| `ResourcePolicyEvaluated` | Server evaluated the join policy against the report. |
| `JoinGateDryRunSent` | Server sent the dry-run join gate decision to the client. |
| `ReadyDryRun` | Full handshake pipeline complete; all steps were dry-run only. |
| `Failed` | Terminal error state; reason stored in `failure_reason`. |

## Transition Rules

Only the forward transitions shown in the state graph are valid. The state
machine enforces:

- No backwards transitions.
- No skipped states.
- `Failed` is terminal — no further transitions are accepted.
- Invalid transitions return `SessionStateError::InvalidTransition`.

## Error Variants (`SessionStateError`)

| Variant | Meaning |
|---|---|
| `InvalidTransition { from, to }` | Attempted transition is not in the valid graph. |
| `ProtocolMismatch { client, server }` | Client version does not match the server's exact version. Transitions to `Failed`. |
| `MissingAnnouncement` | Reserved: resource announcement was expected but not present. |
| `MissingAvailabilityReport` | Reserved: availability report was expected but not received. |
| `PolicyBlockedDryRun` | Resource policy would block the client. State still transitions; error is a log signal only. No disconnect in dry-run mode. |
| `InternalError(String)` | Catch-all for unexpected internal failures. |

## Current Dry-Run Behaviour

All session states are tracked and logged. No enforcement is applied:

- **`ProtocolMismatch`:** The server sends a `Disconnect` message as before. The
  state machine records `Failed` for traceability. This is the only enforced
  disconnect — it predates the session state machine and is unchanged.

- **`PolicyBlockedDryRun`:** The resource policy may evaluate to `Blocked`. The
  state machine records this and returns `Err(PolicyBlockedDryRun)` as a signal
  to the caller. The session continues to `JoinGateDryRunSent` → `ReadyDryRun`
  regardless. No client is disconnected.

- **`ReadyDryRun`:** Reaching this state means the full handshake pipeline ran
  without enforced failure. It does not mean the client is authorised to play —
  it means all dry-run checks passed their reporting stage.

## Relation to Existing Features

| Feature | Session state |
|---|---|
| Protocol version check | `Connected → HelloReceived → VersionChecked` or `Failed` |
| Dry-run negotiation | `VersionChecked → NegotiationDryRunLogged` |
| Capability gate logging | Logged alongside `NegotiationDryRunLogged` and `JoinGateDryRunSent` |
| Resource announcement | `NegotiationDryRunLogged → ResourceAnnouncementSent` |
| Resource availability report | `ResourceAnnouncementSent → AvailabilityReportReceived` |
| Policy evaluation | `AvailabilityReportReceived → ResourcePolicyEvaluated` |
| Join gate dry-run | `ResourcePolicyEvaluated → JoinGateDryRunSent` |
| Session complete | `JoinGateDryRunSent → ReadyDryRun` |

## Future Enforcement Point

When active enforcement is introduced (a future milestone):

- `ProtocolMismatch` already disconnects.
- `PolicyBlockedDryRun` can be promoted to a disconnect by returning early when
  `on_policy_evaluated` returns `Err(PolicyBlockedDryRun)`.
- `ReadyDryRun` will be replaced or supplemented by an enforced `Ready` state.
- `JoinGateMode::Enforced` (currently unused) will gate the transition out of
  `JoinGateDryRunSent`.

No code changes are needed to the state machine itself for these scenarios — only
the server handler needs to act on the returned errors rather than logging them.

## Hard Boundaries

This feature does not and will not:

- Disconnect clients based on capability checks (still report-only).
- Enforce the join gate (still dry-run).
- Add downloads, file serving, or resource repair.
- Add script execution or any scripting runtime.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All session state logic is clean-room, deterministic, and produces no
side-effects beyond structured log output at `info` and `warn` level.
