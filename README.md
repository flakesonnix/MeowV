# MeowV

MeowV is a clean-room, standalone multiplayer prototype inspired by GTA V multiplayer frameworks while intentionally avoiding GTA V integration, anti-cheat bypassing, DRM workarounds, proprietary protocols, and Rockstar online services.

Milestone 0 provides:

- Rust workspace
- shared protocol crate
- async server
- dummy client
- protocol version handshake
- login, chat, and fake entity position sync packets
- tick loop
- structured logging
- TOML config loading
- Nix flake development shell

## Legal Scope

This repository is for clean-room architecture and safe prototyping only.

- No game memory hooks
- No injection
- No reverse-engineered proprietary protocol implementation
- No Rockstar service integration

Read `docs/legal-boundaries.md` before expanding scope.

## Quick Start

### Nix

```bash
nix develop
cargo run -p server
```

In another terminal:

```bash
nix develop
cargo run -p client -- --config example.client.toml --name alice --message "hello world"
```

Formatting and linting inside Nix shell:

```bash
nix develop
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

### Plain Rust

```bash
cargo run -p server
cargo run -p client -- --config example.client.toml --name alice --message "hello world"
```

## Config

Server defaults:

- bind address: `127.0.0.1:7000`
- tick rate: `10`

Override with environment variables:

```bash
MEOWV_SERVER_BIND=127.0.0.1:7001 MEOWV_TICK_RATE=20 cargo run -p server
```

Use example config file:

```bash
MEOWV_CONFIG=example.server.toml cargo run -p server
```

Client example config:

```bash
MEOWV_CLIENT_CONFIG=example.client.toml cargo run -p client
```

Override precedence for client config:

- defaults
- config file
- environment variables
- CLI flags

## Workspace Commands

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
```

## Protocol Notes

- Current prototype protocol version: `1`
- Login must include `protocol_version`
- Server rejects mismatched versions with `disconnect` packet

## Milestone 0 Notes

- Networking uses newline-delimited JSON for readability and reviewability.
- Packet format is intentionally simple and not optimized.
- Fake entity sync is simulated server state, not tied to any game engine.
- TODO: keep any future GTA V-specific bridge in isolated crate with explicit legal review gate.
