Client periodic heartbeat

Flags:

- --heartbeat-enabled
- --heartbeat-interval-ms <n>   # interval between pings in milliseconds (default 5000)
- --heartbeat-timeout-ms <n>    # timeout waiting for a Pong in milliseconds (default 2000)
- --heartbeat-policy <value>    # "report_only" (default) or "strict"

Behavior:

- After normal login, when --heartbeat-enabled is set the client starts a background
  loop that sends ClientMessage::Ping { sequence } and waits for ServerMessage::Pong { sequence }.
- Sequence starts at 1 and increments on each ping.
- On Pong: prints/logs "Heartbeat <n>: Pong received".
- On timeout/error: prints/logs "Heartbeat <n>: failed: <error>".
- Under `ReportOnly` (default): continues after every timeout — no enforcement disconnect.
- Under `Strict`: disconnects when `timeout_or_error_count >= CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD (3)`.
- On clean Ctrl-C shutdown (or after enforcement disconnect), client prints deterministic heartbeat summary with sent/pong/timeout counts and last ping/pong sequence numbers.

Observability:

- Client tracks: `heartbeat_sent_count`, `heartbeat_pong_count`, `heartbeat_timeout_or_error_count`, `last_ping_sequence`, `last_pong_sequence`.
- Server session diagnostics track heartbeat event counts derived from event log: `ping_received_count`, `pong_sent_count`.
- Server admin `sessions` and `diagnostics` commands show `ping_rx=N` and `pong_tx=N` per session in the live registry snapshot. Zero counts appear explicitly when no heartbeat activity has occurred.
- No heartbeat enforcement or disconnect behavior is added by these metrics.

Policy Planner (M4.11):

- `HeartbeatPolicy`: `ReportOnly` (default) or `Strict`.
- `HeartbeatPlannerInput`: `ping_sent`, `pong_received`, `timeout_or_error`.
- `evaluate_heartbeat()` returns a deterministic `HeartbeatDecision`:
  - `NoHeartbeatObserved` — no pings sent
  - `Healthy` — all pings answered, no errors
  - `WouldWarnNoPongYet` — pings sent, no pong yet, no timeout recorded
  - `WouldWarnTimeout` — one or more timeouts/errors (both policies below disconnect threshold)
  - `WouldMarkUnhealthy` — pong gap with no recorded timeout (server-only view)
  - `WouldDisconnectMissedHeartbeat` — `Strict` only, `timeout_or_error >= 3`
- Server builds diagnostics with `with_heartbeat_policy(&config.heartbeat.policy)` — decision appears in diagnostic log output.
- Server-only view uses `timeout_or_error = 0` (client-side timeout counts are not reported back to server).
- This is planning only — no actual disconnect or enforcement occurs in this milestone.

Config Plumbing (M4.13):

- `[heartbeat]` section in `example.server.toml` with `policy = "report_only"` (default) or `"strict"`.
- `HeartbeatSection` struct in `config.rs` with `Default → ReportOnly`.
- Configured policy is set on `SessionRegistry` via `set_heartbeat_policy()` at startup.
- Registry snapshot carries the policy; `to_diagnostics_text()` evaluates labels under the configured policy.
- `ServerRuntimeStatus::to_text()` includes `heartbeat_policy: <value>` for operator inspection.
- Startup lifecycle summary includes `heartbeat_policy:` line.
- No disconnect enforcement occurs regardless of policy setting in this milestone.

Client Enforcement (M4.14):

- `ClientHeartbeatPolicy` enum (`ReportOnly`, `Strict`) in `client/src/lib.rs`.
- `CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD = 3` — matches server-side `MISSED_HEARTBEAT_DISCONNECT_THRESHOLD`.
- `heartbeat_loop` accepts `policy: ClientHeartbeatPolicy`; breaks with `enforcement_disconnect=true` at threshold under `Strict`.
- `HeartbeatMetrics.enforcement_disconnect: bool` — set when enforcement triggered exit.
- `to_text()` appends `heartbeat_enforcement_disconnect: true` when set.
- `--heartbeat-policy strict` enables enforcement; default is `report_only`.
- Server-side labels (`heartbeat=<label>`) remain observational — server view always has `timeout_or_error=0` and cannot trigger `WouldDisconnectMissedHeartbeat` without client-reported data.
- No protocol wire changes; registry cleanup on disconnect works via `SessionGuard` on all paths.

Server-Initiated Heartbeat Protocol Stub (M4.16):

Two heartbeat directions now exist in the protocol:

| Direction | Ping message | Pong message | Owner | Purpose |
|-----------|-------------|-------------|-------|---------|
| Client-initiated | `ClientMessage::Ping { sequence }` | `ServerMessage::Pong { sequence }` | Client | Client diagnostics, manual liveness check |
| Server-initiated | `ServerMessage::ServerPing { sequence }` | `ClientMessage::ServerPong { sequence }` | Server | Future authoritative liveness enforcement |

Client-initiated direction (existing):
- Client sends `Ping`, server echoes `Pong`.
- Client tracks timeouts; `timeout_or_error_count` lives on client side only.
- Server cannot enforce via this direction (server always has `timeout_or_error=0`).
- `Strict` enforcement is client-side self-disconnect.

Server-initiated direction (M4.16 stub — inert):
- `ServerMessage::ServerPing { sequence: u64 }` and `ClientMessage::ServerPong { sequence: u64 }` added as inert DTOs.
- Wire type tags: `"server_ping"` and `"server_pong"` — no collision with `"ping"` / `"pong"`.
- Server handler arm logs receipt at `info` level; no timer, no enforcement, no tracking.
- No client-side reply behavior yet — added in M4.17.

Client Responds to ServerPing (M4.17):

- `client::handle_server_ping(writer, sequence)` — public async fn in `lib.rs`; sends `ClientMessage::ServerPong { sequence }`.
- Main receive loop in `main.rs` handles `ServerMessage::ServerPing { sequence }` arm: replies with `ServerPong` and logs at `info`.
- `heartbeat::send_ping_and_wait_with_timeout` intercepts `ServerPing` while waiting for a client-initiated `Pong`: replies inline, then continues waiting for the matching `Pong`. Sequence fidelity is preserved — each `ServerPong` echoes the `ServerPing` sequence exactly.
- Future milestone (M4.18+) will wire the server-side scheduler: server sends `ServerPing` on interval, tracks missed `ServerPong` replies, disconnects under `Strict` when threshold reached.
- This is the authoritative liveness path: server owns the timer and measures directly — no trust assumption on client-reported data.

Server-Side ServerPing Scheduler (M4.18):

- `HeartbeatSection.server_ping_interval_ms: u64` — new config field; default 5000 ms; `0` disables server-initiated pings entirely.
- `example.server.toml` gains `server_ping_interval_ms = 5000` under `[heartbeat]`.
- `SessionEventKind::ServerPingSent` and `SessionEventKind::ServerPongReceived` — two new event log kinds for audit trail.
- `SessionRegistryEntry` gains `server_ping_sent_count: usize` and `server_pong_received_count: usize`; surfaced as `srv_ping_tx=N  srv_pong_rx=N` in `to_diagnostics_text()`.
- `SessionRegistry::update_server_heartbeat_counts()` — targeted update method; called after every `ServerPingSent` and `ServerPongReceived` event.
- `SessionDiagnostics` gains `server_ping_sent_count` and `server_pong_received_count`; populated from event log; appear in `to_text()` and `to_json_stub()`.
- Scheduler runs inside `handle_client` post-handshake `select!` loop: `interval_at(now + dur, dur)` schedules first tick after one full interval (no t=0 fire); `MissedTickBehavior::Delay` prevents burst catch-up.
- `if srv_ping_enabled` guard on the tick branch disables it entirely when `server_ping_interval_ms == 0` — future is not polled when disabled.
- `ServerPong` replies are validated for sequence fidelity at `info` level only — mismatch logged but not fatal under `ReportOnly` policy.
- No disconnect enforcement in this milestone regardless of policy. `Strict` enforcement for missed server pongs is a future milestone.

Server-Side Heartbeat Timeout Status / Planner (M4.19):

- `ServerHeartbeatPlannerInput { pings_sent: u64, pongs_received: u64 }` — pure planner input for server-initiated direction; derived from `server_ping_sent_count` and `server_pong_received_count`.
- `MISSED_SERVER_PONG_DISCONNECT_THRESHOLD = 3` — threshold for `WouldDisconnect` decision under `Strict`; mirrors client-side threshold.
- `ServerHeartbeatDecision` variants and `srv_heartbeat=<label>` short labels:
  - `NoActivity` / `"no_activity"` — no `ServerPing` sent yet
  - `Healthy` / `"healthy"` — all pings answered
  - `AwaitingPong` / `"awaiting_pong"` — pings sent, no pong ever received, below threshold
  - `MissedPong` / `"missed_pong"` — some pong received but gap present, below threshold
  - `WouldDisconnect` / `"would_disconnect"` — `Strict` only, `missed >= MISSED_SERVER_PONG_DISCONNECT_THRESHOLD`
- `evaluate_server_heartbeat(input, policy)` — pure deterministic function; `ReportOnly` never escalates to `WouldDisconnect`.
- `SessionRegistrySnapshot::to_diagnostics_text()` extended with `srv_heartbeat=<label>` per session; admin `sessions` output includes the label automatically.
- `SessionDiagnostics::with_heartbeat_policy()` now evaluates both directions; `server_heartbeat_decision: Option<String>` field emitted as `server_heartbeat_decision: <label>` in `to_text()` and included in `to_json_stub()`.
- No enforcement or actual disconnect in this milestone; `WouldDisconnect` is a planning-only label.

Strict Server-Side Heartbeat Enforcement (M4.20):

- Enforcement runs in the `srv_ping_interval.tick()` arm of the post-handshake `select!` loop.
- After each `ServerPing` is sent and counts recorded, `evaluate_server_heartbeat` is called under `Strict` policy.
- On `WouldDisconnect` (missed ≥ `MISSED_SERVER_PONG_DISCONNECT_THRESHOLD`): `session.fail(reason)` transitions the session state machine to `Failed`; `SessionEventKind::Failed` recorded; registry updated to `Failed`; optional structured diagnostics emitted; `Disconnect` sent to client (best-effort via writer channel); `break` exits main loop.
- `SessionGuard` RAII removes the session from the registry on handler exit — all exit paths covered.
- Under `ReportOnly` the enforcement block is skipped entirely; behavior is identical to M4.19.
- `srv_heartbeat=would_disconnect` label is transient under `Strict`: the session is removed before it can be read in a stable registry snapshot.

Enforcement Invariants (M4.21):

Two disconnect paths exist with different delivery guarantees:

- Handshake-phase enforcement (`handle_enforcement`): called before `writer_task` is spawned; uses `send_direct(writer, Disconnect)` — writes and flushes directly to the TCP half; Disconnect delivery is **guaranteed**.
- Session-loop enforcement (scheduler tick): called after `writer_task` is spawned; uses `client_tx.send(Disconnect)` to queue the frame, then `break` exits the loop; after loop exit `writer_task.abort()` drops `writer_half`, closing the TCP write half. Disconnect frame delivery is **best-effort** (abort may preempt delivery). TCP close (EOF) is **guaranteed** via `writer_half` drop.

Client-side disconnect detection must rely on EOF, not on receiving a `Disconnect` frame. The two heartbeat directions (client-initiated Ping/Pong vs server-initiated ServerPing/ServerPong) are entirely independent — enforcement on one direction does not affect the other.
