Client periodic heartbeat

Flags:

- --heartbeat-enabled
- --heartbeat-interval-ms <n>   # interval between pings in milliseconds (default 5000)
- --heartbeat-timeout-ms <n>    # timeout waiting for a Pong in milliseconds (default 2000)

Behavior:

- After normal login, when --heartbeat-enabled is set the client starts a background
  loop that sends ClientMessage::Ping { sequence } and waits for ServerMessage::Pong { sequence }.
- Sequence starts at 1 and increments on each ping.
- On Pong: prints/logs "Heartbeat <n>: Pong received".
- On timeout/error: prints/logs "Heartbeat <n>: failed: <error>" and continues.
- No disconnect or enforcement happens on missed pongs — loop is report-only.
- On clean Ctrl-C shutdown, client prints deterministic heartbeat summary with sent/pong/timeout counts and last ping/pong sequence numbers.

Observability:

- Client tracks: `heartbeat_sent_count`, `heartbeat_pong_count`, `heartbeat_timeout_or_error_count`, `last_ping_sequence`, `last_pong_sequence`.
- Server session diagnostics track heartbeat event counts derived from event log: `ping_received_count`, `pong_sent_count`.
- No heartbeat enforcement or disconnect behavior is added by these metrics.
