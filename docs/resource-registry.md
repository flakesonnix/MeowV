# Resource Registry

## Purpose

Milestone 1.0 adds a local-only resource registry that discovers multiple resources, validates manifests, checks dependencies, detects cycles, and computes deterministic load order.

## Current Scope

- local only
- no downloads
- no script execution
- no scripting runtime
- no GTA V integration

## Discovery Rules

- only immediate child directories of resources root are considered
- a resource must contain `resource.toml`
- directories without `resource.toml` are ignored
- resource folder name must match manifest name
- symlinks are rejected
- arbitrary system paths are not scanned

## Dependency Resolution

- missing dependencies are rejected
- duplicate resource names are rejected
- dependency cycles are rejected
- load order is deterministic
- lexical ordering is used when multiple independent resources are ready

## Client Mode

Example:

```bash
cargo run -p client -- --resource-registry examples/resources
```

Behavior:

- discover resources under root
- validate manifests
- print dependencies
- print final load order
- do not execute anything

## Future Boundary

This registry is metadata and ordering only. Any future runtime that actually loads or executes resource entrypoints must stay behind a separate reviewed boundary.

## Clean-Room Note

Dependency resolution and registry layout must remain original. Do not copy proprietary registry formats, loader behavior, or private package orchestration rules from GTA V multiplayer ecosystems.

## Edition Independence

This registry is independent from GTA V Legacy and Enhanced because it only models local package metadata and dependency ordering.
