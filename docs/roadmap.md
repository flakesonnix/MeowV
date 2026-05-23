# Roadmap

## Milestone 0

Standalone prototype:

- Rust workspace
- Nix dev shell
- shared protocol
- server
- dummy client
- login/chat/entity sync

## Milestone 1

Protocol hardening:

- version negotiation
- heartbeat/ping
- disconnect reasons
- better config files
- integration tests

## Milestone 2

Resource/runtime model:

- server resource manifest format
- script/runtime abstraction
- permission model
- hot-reload experiments in standalone environment

## Milestone 3

Transport/runtime refinement:

- snapshot interpolation experiments
- reliability channels
- interest management prototype
- metrics and tracing exports

## Milestone 4

Native boundary evaluation:

- decide whether any low-level bridge is necessary
- if yes, isolate in narrow crate/module
- require legal and architectural review before implementation
