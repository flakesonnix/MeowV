meowv-client manual ping

Usage examples:

- Send a single ping and wait for pong (default sequence=1, timeout=2000ms):

  meowv-client --ping-once

- Specify sequence and timeout:

  meowv-client --ping-once --ping-sequence 42 --ping-timeout-ms 1000

Behavior:

- Connects and performs normal login
- Sends ClientMessage::Ping { sequence }
- Waits for ServerMessage::Pong { sequence }
- Prints "Ping <n>: Pong received" on success
- Prints "Ping <n>: failed: <error>" on timeout or error

No periodic heartbeat or liveness enforcement is performed.
