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

- compatibility policy docs
- heartbeat/ping
- better config files
- integration tests

## Milestone 1.8

Protocol compatibility negotiation design:

- dry-run negotiation data structures
- version ranges, capability flags
- intersection/evaluation logic
- server/client dry-run reporting
- docs explaining future activation path

## Milestone 1.0

Resource registry:

- multi-resource discovery
- dependency validation
- cycle detection
- deterministic load order

## Milestone 1.1

Runtime boundary:

- no-exec load planning
- runtime separation prep
- deterministic resource planning

## Milestone 1.2

Runtime state machine:

- no-exec lifecycle simulation
- deterministic resource states
- dependency readiness checks

## Milestone 0.5

Game edition layer:

- edition-aware metadata types
- conservative placeholder detection
- clean-room support policy docs
- no runtime GTA V integration


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
