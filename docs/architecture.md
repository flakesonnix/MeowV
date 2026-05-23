# Architecture

## Goals

Milestone 0 proves the project skeleton, development environment, and packet flow without touching GTA V internals.

## Current Components

### `crates/protocol`

Shared packet definitions and line-delimited JSON encode/decode helpers.

### `crates/server`

Async TCP server with:

- login handshake
- chat broadcast
- simulated entity state broadcast
- fixed-rate tick loop
- structured logs
- config loading

### `crates/client`

Dummy client used for protocol and UX testing. It connects, logs in, sends one chat message, and prints server packets.

## Data Flow

1. Client connects over TCP.
2. Client sends `ClientMessage::Login`.
3. Server responds with `ServerMessage::Welcome`.
4. Client may send `ClientMessage::Chat`.
5. Server broadcasts chat and periodic fake entity snapshots.

## Safety Boundary

No game integration exists here.

TODO: if future native bridge work starts, keep it in separate low-level crate with strict interface boundary, legal review notes, and no proprietary code or copied data layouts.

## Rust-First Direction

Rust is sufficient for:

- backend server runtime
- protocol evolution
- launcher/tooling
- asset/resource packaging metadata
- standalone client simulation

C++ should only appear later if a narrow, unavoidable platform integration boundary exists.
