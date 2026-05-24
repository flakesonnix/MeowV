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
