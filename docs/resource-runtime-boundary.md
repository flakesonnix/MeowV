# Resource Runtime Boundary

## Purpose

Milestone 1.1 adds a no-exec runtime boundary for resources. It builds a deterministic load plan from the discovered registry and dependency order without executing any scripts.

## Load Plan vs Execution

- load plan describes what would be loaded
- execution is out of scope
- current output is metadata only

`ResourceLoadPlan` contains:

- resource name
- root directory
- phase
- planned server/client entrypoints
- dependency list

## Why No Scripts Are Executed Yet

This milestone is a planning layer only. Deferring execution keeps the boundary auditable and avoids mixing metadata orchestration with runtime behavior too early.

## Current Scope

- local only
- deterministic order from dependency resolution
- no script execution
- no file content reads for entrypoints
- no runtime embedding for Lua, JS, or WASM

## Future Boundary

Future work may add:

- explicit scripting runtime boundary
- server/client runtime separation
- sandboxing and permissions
- runtime startup and shutdown lifecycle

## Sandboxing Considerations

When execution exists later, it should run behind explicit capability limits, filesystem constraints, and clear server/client separation.

## Clean-Room Note

Runtime planning and later loading rules must remain original. Do not copy proprietary loader behavior, startup sequencing, or private runtime orchestration from GTA V multiplayer ecosystems.

## Edition Independence

This runtime boundary is independent from GTA V Legacy and Enhanced because it only models local metadata and planned order. No game-facing behavior exists here.
