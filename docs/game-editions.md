# Game Editions

## Scope

Milestone 0.5 adds edition-aware metadata only. It does not add GTA V integration, memory access, hooks, injection, or platform bypass behavior.

## Current Status

- GTA V Legacy support status: unknown, research only
- GTA V Enhanced support status: unknown, research only
- current recommendation: keep core server, protocol, and resource model edition-agnostic

## Why This Exists Now

Users may have different installed editions. Project should model that safely without pretending runtime compatibility is already known.

Current `game_edition` crate only provides:

- `GameEdition`
- `GamePlatform`
- `GameBuildInfo`
- conservative placeholder detection helpers

## Detection Policy

- if detection is uncertain, return `Unknown`
- do not claim Enhanced support from filename alone
- do not claim Legacy support from filename alone
- placeholder detection is metadata only, not compatibility proof

## Future Integration Boundary

Any future edition-specific runtime bridge must stay behind a narrow boundary and pass legal and architectural review before implementation.

TODO areas for future review:

- filesystem-only install discovery rules per platform
- user-provided game path validation
- signed binary metadata checks if legally safe and technically justified

## Clean-Room Warning

Do not use leaked offsets, copied launch flows, proprietary manifests, or reverse-engineered private implementation details when extending edition detection.

## Why Standalone Prototype Stays Independent

Milestone 0 server and dummy client remain independent of GTA V edition because networking, protocol design, config, resource ideas, and developer workflow can all be validated without touching game-specific behavior.
