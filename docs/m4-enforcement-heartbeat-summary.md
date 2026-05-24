# M4 Enforcement and Heartbeat Summary (M4.22)

Concise checkpoint for M4.0-M4.21. This doc records current behavior and
guarantees only. M4.22 changes no runtime or protocol behavior.

## Policy Axes

Three independent policy axes exist:

| Area | Config / flag | Default | Strict effect |
|------|---------------|---------|---------------|
| Session enforcement | `[enforcement] mode` | `report_only` | Disconnect on non-Allow handshake/session enforcement decisions |
| Signature enforcement | client `--signature-policy` and trusted keys | `report_only` | Client rejects unsigned/invalid resource announcements |
| Server heartbeat enforcement | `[heartbeat] policy` | `report_only` | Server disconnects when authoritative server heartbeat reaches `WouldDisconnect` |

## Session Enforcement Status

- Hard failures always disconnect: non-Login first message, invalid login transition, protocol version mismatch.
- `ReportOnly`: enforcement decisions logged in diagnostics; no extra disconnect.
- `Strict`: invalid handshake/session transitions disconnect with structured failure reason.
- Join gate and protocol negotiation remain dry-run only. They do not enforce disconnects.

## Signature Policy Status

- Real Ed25519 verification exists.
- `ReportOnly`: invalid/unsigned announcements reported, not rejected.
- `Strict`: client rejects invalid/unsigned announcements.
- Strict requires trusted keys. No silent fallback.
- Signature gate is client-side in this stack checkpoint.

## Heartbeat Status

Two heartbeat directions exist. They are independent.

| Direction | Messages | Role | Authority |
|-----------|----------|------|-----------|
| Client-initiated | `Ping` / `Pong` | Manual + diagnostic client liveness | Not authoritative for server enforcement |
| Server-initiated | `ServerPing` / `ServerPong` | Server-owned liveness check | Authoritative for server enforcement |

### Client-Initiated Heartbeat

- Supports `--ping-once` and periodic heartbeat loop.
- Tracks sent/pong/timeout metrics and prints deterministic summary on clean shutdown.
- Client strict mode may self-disconnect after repeated timeout/error threshold.
- This direction is diagnostic/manual from server perspective.
- Client Ping activity does **not** keep session alive under strict server heartbeat enforcement.

### Server-Authoritative Heartbeat

- Server sends `ServerPing` on configured interval.
- Client replies with matching `ServerPong`.
- Server tracks `srv_ping_tx`, `srv_pong_rx`, `srv_heartbeat=<label>`.
- `ReportOnly`: labels and diagnostics only.
- `Strict`: `WouldDisconnect` causes session failure + loop exit + TCP close.

## Admin and Diagnostics Visibility

- Admin `status`: policy values and runtime status.
- Admin `sessions`: per-session counts and heartbeat labels.
- Admin `diagnostics`: session diagnostics including enforcement and heartbeat decisions.
- Session diagnostics expose event-derived heartbeat counts:
  - `ping_received_count`
  - `pong_sent_count`
  - `server_ping_sent_count`
  - `server_pong_received_count`
- No IP addresses or personal data in diagnostic/admin output.

## Disconnect Guarantees

Two disconnect paths exist:

1. Handshake-phase direct-write path
- `Disconnect` frame delivery guaranteed.

2. Post-spawn session-loop path
- `Disconnect` frame best-effort only.
- Writer task abort may race queued frame delivery.
- TCP close / EOF guaranteed.

Authoritative guarantee under strict server heartbeat enforcement: TCP close / EOF.
`Disconnect` frame is advisory when sent through writer task channel.

## Important Invariants

- Client `Ping`/`Pong` is diagnostic/manual heartbeat.
- Server `ServerPing`/`ServerPong` is authoritative server liveness.
- Client Ping traffic must not keep session alive under strict server heartbeat enforcement.
- Strict server heartbeat enforcement guarantees TCP close / EOF.
- Disconnect frame delivery may race writer task abort in post-spawn enforcement path.

## Intentionally Not Implemented

- Protocol negotiation enforcement
- Join gate enforcement
- Server-side trust in client-reported heartbeat health
- Remote admin interface
- Downloads, cache repair, file serving, or resource execution
- GTA V integration

## Related Docs

- `docs/client-heartbeat.md`
- `docs/heartbeat-authority-design.md`
- `docs/live-session-enforcement.md`
- `docs/server-policy-configuration.md`
- `docs/server-admin-debug-commands.md`
- `docs/strict-signature-policy.md`
