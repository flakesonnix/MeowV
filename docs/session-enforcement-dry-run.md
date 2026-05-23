# Session Enforcement Dry-Run

## Purpose

Session enforcement maps session handshake outcomes to a deterministic
decision/report describing what the server would do under a strict enforcement
policy, without actually changing live runtime behavior.

This is a pure planning layer. The evaluation function
(`evaluate_enforcement`) consumes session state machine state and a policy
setting, and returns a `SessionEnforcementDecision`. No I/O, no side effects,
no network exposure.

## Policy Modes

| Mode | Behavior |
|------|----------|
| `ReportOnly` | Always returns `Allow`. No enforcement, no decisions. |
| `Strict` | Returns the enforcement decision matching the session outcome. |

`ReportOnly` is the default. `Strict` is the target for future enforcement
milestones.

## Decision Outcomes

| Decision | Meaning |
|----------|---------|
| `Allow` | Session handshake completed successfully. |
| `WouldDisconnectInvalidFirstMessage` | First packet was not a Login message. |
| `WouldDisconnectVersionMismatch` | Client protocol version does not match server. |
| `WouldDisconnectCapabilityGateFailure` | A required capability is missing. |
| `WouldDisconnectInvalidStateTransition` | Session state machine received an illegal transition. |
| `WouldMarkSessionFailed` | Session failed for other reasons (connection reset, handshake incomplete, etc.). |

## Evaluation Logic (Strict)

```
current_state          → decision
─────────────────────────────────────────────
ReadyDryRun            → Allow
Connected              → WouldDisconnectInvalidFirstMessage
Failed (version)       → WouldDisconnectVersionMismatch
Failed (transition)    → WouldDisconnectInvalidStateTransition
Failed (other)         → WouldMarkSessionFailed
other intermediate     → WouldMarkSessionFailed
```

Under `ReportOnly` all inputs return `Allow`.

## Text Output

Each decision implements `to_text()` producing a deterministic single-line
string:

```
decision: allow
decision: would_disconnect invalid_first_message
decision: would_disconnect version_mismatch client=99 server=1
decision: would_disconnect capability_gate_failure capability=ResourceAnnouncement
decision: would_disconnect invalid_state_transition from=HelloReceived to=ReadyDryRun
decision: would_mark_session_failed reason=connection reset by peer
```

## Hard Boundaries

- No live behavior change. Decisions are pure values, not executed.
- No protocol wire format change.
- No network exposure of decisions.
- No IP addresses, timestamps, or personal data in decision output.
- No downloads, cache writes, or resource execution.

## Integration Path (Future)

The decision layer feeds into:

1. Admin command output (show current enforcement evaluation per session)
2. Diagnostics/debug dump (include enforcement decision)
3. Live enforcement (M3.1+) — actually disconnect or mark failed

## Types

Located in `crates/server/src/enforcement.rs`:

| Symbol | Kind |
|--------|------|
| `SessionEnforcementPolicy` | enum (ReportOnly, Strict) |
| `SessionEnforcementDecision` | enum (6 variants) |
| `evaluate_enforcement` | pure function |
| `SessionEnforcementDecision::to_text` | display method |
