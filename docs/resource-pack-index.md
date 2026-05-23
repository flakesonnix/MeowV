# Resource Pack Index

## Purpose

Milestone 0.8 adds local-only resource pack indexing for resource directories. It loads a manifest, scans regular files, computes SHA-256 hashes, and builds an in-memory index.

## Current Scope

- local only
- no downloads
- no script execution
- no scripting runtime
- no GTA V integration
- no on-disk index file yet

## Why SHA-256

SHA-256 is widely supported, deterministic, and suitable for content identity and future cache verification. Current output is lowercase hexadecimal.

## Current Resource Root

A resource directory contains:

- `resource.toml`
- optional files in subdirectories

Example:

- `examples/resources/chat/resource.toml`
- `examples/resources/chat/server/main.lua`
- `examples/resources/chat/client/main.lua`
- `examples/resources/chat/README.md`

## Index Model

- `ResourcePackIndex`
- `ResourceFileEntry`

Each file entry stores:

- relative path
- size in bytes
- SHA-256 hash

## Rules

- include only regular files
- include `resource.toml`
- ignore directories
- reject symlinks
- do not follow symlinks
- use only paths relative to resource root
- reject absolute paths
- reject `..` traversal
- sort entries deterministically by relative path

## Client Inspection Mode

Example:

```bash
cargo run -p client -- --resource-index examples/resources/chat
```

Behavior:

- load `resource.toml`
- validate manifest
- scan files
- compute SHA-256 hashes
- print readable summary
- do not execute anything

## Why Symlinks Are Rejected Initially

Rejecting symlinks avoids ambiguous file ownership and traversal behavior while rules are still small and easy to audit.

## Future Work

- JSON export format for index files
- signed resource downloads
- cache verification
- incremental hashing
- trust policy for remote distribution

## Clean-Room Note

Resource indexing and hash workflows must remain original. Do not copy proprietary resource packaging schemes or private distribution metadata from GTA V multiplayer ecosystems.

## Edition Independence

This indexing layer is independent from GTA V Legacy and Enhanced because it only models local files and metadata. Any edition-specific runtime loading belongs behind a later reviewed integration boundary.
