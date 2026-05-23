# Resource Runtime State Machine

## Purpose

Milestone 1.2 adds a local-only, no-exec resource runtime state machine. It tracks lifecycle state transitions for resources derived from an existing `ResourceLoadPlan`.

## Allowed Transitions

- `Planned -> Validated`
- `Validated -> Ready`
- `Ready -> Started`
- `Started -> Stopped`
- `Stopped -> Ready`
- any non-terminal state may move to `Failed`

`Failed` is terminal in this milestone.

## Dependency Rule

A resource may only move to `Started` when all dependencies are already `Ready` or `Started`.

## Why Started Is Still No-Exec

`Started` in this milestone means lifecycle intent only. It records that the resource passed dependency checks and would be considered started by the control layer. No scripts are executed.

## Current Scope

- local only
- deterministic ordering
- no script execution
- no file content reads for entrypoints
- no process spawning

## Future Runtime Boundary

Future work may add:

- actual runtime startup hooks
- explicit server/client runtime split
- failure recovery policies
- resource restart orchestration

## Sandboxing Considerations

When a real runtime exists later, it should include capability limits, lifecycle isolation, and explicit failure boundaries between resources.

## Clean-Room Note

Lifecycle rules and runtime control flow must remain original. Do not copy proprietary resource supervisor behavior, startup ordering logic, or internal runtime models from GTA V multiplayer ecosystems.

## Edition Independence

This state machine is independent from GTA V Legacy and Enhanced because it only models local resource lifecycle metadata and dependency readiness.
