# Resource Manifest

## Purpose

Milestone 0.7 adds a clean-room, GTA-independent resource manifest foundation. It describes package metadata only. It does not execute scripts, download remote code, or integrate with GTA V.

## Format

- TOML file per resource
- current example: `examples/resources/chat/resource.toml`

Current fields:

- `name`
- `version`
- `description`
- `authors`
- `license`
- `entrypoints.server`
- `entrypoints.client`
- `dependencies`
- `tags`
- `protocol_version`
- `edition_compatibility`

## Validation Rules

- name must not be empty
- version must not be empty
- resource names use lowercase letters, numbers, dash, underscore only
- dependency names follow same resource-name rules
- `protocol_version` must match current protocol policy
- entrypoint paths must be relative
- absolute paths rejected
- `..` traversal rejected

## Client Inspection Mode

Example:

```bash
cargo run -p client -- --resource-manifest examples/resources/chat/resource.toml
```

Behavior:

- load manifest
- validate manifest
- print summary
- do not execute anything

## Future Work

- resource downloading
- signature and trust metadata
- dependency resolution strategy
- isolated scripting runtime boundary
- server/client resource loading rules

## Boundaries

- no remote code execution yet
- no scripting runtime yet
- no Rockstar services involved
- no copied proprietary manifest formats

## Clean-Room Note

Manifest layout and validation must remain original. Do not copy private or proprietary package schemas from GTA V multiplayer ecosystems.

## Edition Independence

This manifest system stays independent from GTA V Legacy and Enhanced because it models portable metadata only. Edition-specific runtime handling, if ever needed, belongs behind a later integration boundary.
