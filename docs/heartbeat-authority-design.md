# Heartbeat Authority Design (Milestone 4.15)

## Problem Statement

After M4.14, strict heartbeat enforcement lives on the client. The client
tracks `timeout_or_error_count` and self-disconnects when the threshold is
reached. The server's heartbeat labels (`heartbeat=<label>`) are observational
only — the server cannot produce `WouldDisconnectMissedHeartbeat` from its own
data because it never receives client-side timeout reports.

This document explains why, compares the two main solutions, and recommends
the authoritative enforcement path for a future milestone.

---

## Why Server-Side Missed-Pong Enforcement Is Not Possible Today

The current protocol is **client-initiated**:

```
Client  →  Ping { sequence }
Server  →  Pong { sequence }   (echo)
```

From the server's view:

| What the server knows | How it knows it |
|-----------------------|-----------------|
| A Ping was received | `SessionEventKind::PingReceived` |
| A Pong was sent | `SessionEventKind::PongSent` |
| Pong was _received by the client_ | **Unknown** — TCP delivery is not confirmed at application layer |
| Client experienced a timeout | **Unknown** — no protocol message carries this |

Because the server always sends a Pong for every Ping it receives, its view is:
- `ping_received_count == pong_sent_count` (always equal, barring send errors)
- `timeout_or_error = 0` in the `HeartbeatPlannerInput` built server-side
- `WouldDisconnectMissedHeartbeat` is therefore unreachable from the server

The client may be sending Pings, timing out while waiting for the Pong, and
accumulating `timeout_or_error_count` — but the server has no window into this.

---

## Option A: Client-Reported Heartbeat Health

Add a new client message carrying the client's own heartbeat metrics:

```rust
ClientMessage::HeartbeatHealthReport {
    sent_count: u64,
    pong_count: u64,
    timeout_or_error_count: u64,
    last_ping_sequence: Option<u64>,
    last_pong_sequence: Option<u64>,
}
```

The client sends this periodically or on each timeout. The server records and
surfaces these counts alongside its own event-log-derived counts.

### Advantages
- Low implementation complexity; no protocol direction change
- Gives server visibility into client-side timeout experience
- Useful for diagnostics: operator can see "client reports 5 missed pongs"
- No timer needed on the server

### Trust Implications

**Client-reported heartbeat metrics are not authoritative for enforcement.**

A well-behaved client reports accurately. A malicious or buggy client can:
- Under-report timeouts (appear healthy when it is not)
- Over-report timeouts (trigger enforcement of a healthy peer)
- Stop sending reports entirely

Using `HeartbeatHealthReport` for disconnect decisions is equivalent to
asking a client whether it should be disconnected. This is not a sound
security or liveness model.

### Verdict: diagnostics only

Client-reported health is appropriate as a **read-only diagnostic field**
surfaced in `diagnostics` and `sessions` admin output alongside server-derived
counts. It must not be the basis for enforcement disconnect decisions.

---

## Option B: Server-Initiated Ping / Server-Side Timeout Tracking

Add a new message pair where the server initiates and the client must respond:

```rust
// New: server → client
ServerMessage::ServerPing { sequence: u64 }

// New: client → server (reply)
ClientMessage::ServerPong { sequence: u64 }
```

The server:
1. Sends `ServerPing` periodically (configurable interval)
2. Waits for a matching `ServerPong` within a timeout window
3. Tracks: `server_ping_sent`, `server_pong_received`, `server_timeout_count`
4. Under `Strict` policy, disconnects when `server_timeout_count >= threshold`

### Advantages
- Server measures liveness directly — no trust assumption on client data
- `WouldDisconnectMissedHeartbeat` becomes reachable server-side
- Enables authoritative disconnect under Strict policy
- Naturally complementary to the existing client-initiated Ping/Pong
  (both can coexist for different observability angles)

### Implementation Notes
- Requires a background timer per session (or a shared server-wide ping scheduler)
- Per-session async task or `tokio::time::interval` inside `handle_client`
- No broad refactor needed if timer is embedded in the existing session loop
- Protocol version may need a capability flag so older clients know to handle `ServerPing`

### Trust Implications
- Fully server-authoritative: the server controls what it sends and when
- A malicious client can ignore `ServerPing` — that is exactly the timeout that
  triggers enforcement, so the model is self-consistent
- No reliance on client-reported data for enforcement decisions

### Verdict: recommended path for future enforcement

Server-initiated Ping/Pong is the correct architecture for authoritative
liveness enforcement. The server observes what it needs directly; the trust
model is sound.

---

## Comparison Summary

| Property | Client-Reported Health | Server-Initiated Ping |
|----------|------------------------|----------------------|
| Protocol change | New client message | Two new messages, new direction |
| Server trust | Not authoritative | Authoritative |
| Enforcement basis | No | Yes |
| Diagnostic value | High | High |
| Complexity | Low | Moderate |
| Timer required | No | Yes (per session or shared) |
| Recommended for disconnect | No | Yes |

---

## Recommended Implementation Path

### M4.15 (this document) — Authority design, no live changes
Establish the design on paper. No protocol changes, no new enforcement.

### M4.16 (future) — Client-reported health as diagnostic only
If useful for operators, add `ClientMessage::HeartbeatHealthReport` as a
report-only diagnostic input. Server records and surfaces in diagnostics/admin.
No enforcement. Clear documentation that this is untrusted input.

### M4.17 (future) — Server-initiated Ping/Pong
Add `ServerMessage::ServerPing` + `ClientMessage::ServerPong`. Server tracks
per-session ping/pong with timeout. Under `Strict` heartbeat policy, disconnect
when server-side timeout count reaches threshold. This is the authoritative
enforcement milestone.

---

## Current State (M4.14)

- Client self-disconnects under `Strict` when its own `timeout_or_error_count >= 3`.
- This is a client-side enforcement: correct behaviour by well-behaved clients.
- Server labels remain observational. No server-side disconnect for heartbeat.
- `HeartbeatDecision::WouldDisconnectMissedHeartbeat` is unused in live code
  paths (only reachable if `timeout_or_error > 0`, which the server never has).

---

## Hard Boundaries

- No protocol wire changes in this milestone.
- No live enforcement changes.
- No resource/cache/signature changes.
- No client disconnect triggered by this design document.
