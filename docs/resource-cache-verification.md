# Resource Cache Verification

## Purpose

Milestone 0.9 adds local-only cache verification for resource packs. It compares a built `ResourcePackIndex` against files already present in a cache directory.

## Current Scope

- local only
- no downloads
- no cache repair
- no script execution
- no scripting runtime
- no GTA V integration

## Verification Rules

- use relative paths from `ResourcePackIndex`
- do not scan extra cache files yet
- missing files report `Missing`
- size mismatch reports `SizeMismatch`
- hash mismatch reports `HashMismatch`
- matching size and hash reports `Valid`
- reject symlinks
- do not follow symlinks
- keep deterministic report ordering

## Why Size Is Checked Before Hash

Size comparison is cheaper than hashing. If file size already differs, verification can stop early for that file and report a deterministic mismatch without extra hashing work.

## Client Mode

Example:

```bash
cargo run -p client -- --verify-cache examples/resources/chat examples/cache/chat-valid
```

Behavior:

- build resource index from resource directory
- verify cache directory against that index
- print counts and per-file status
- print `OK` or `FAILED`
- do not copy, download, repair, or execute anything

## Why Symlinks Are Rejected

Symlink rejection keeps cache verification small, explicit, and resistant to ambiguous path ownership or traversal behavior.

## Future Work

- cache repair step
- local cache population step
- signed resource pack metadata
- signed manifest/resource index verification
- remote distribution policy and trust chain

## Clean-Room Note

Cache verification logic must remain original. Do not copy proprietary cache formats, patch flows, or distribution metadata from GTA V multiplayer ecosystems.

## Edition Independence

This cache verification layer is independent from GTA V Legacy and Enhanced because it checks local files and hashes only. Any edition-specific runtime handling remains outside this milestone.
