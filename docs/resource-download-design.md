# Resource Download Design

## Scope

This document is a **future-only design specification**. No download, file
serving, cache repair, or resource transfer mechanism is implemented in the
current milestone. Local cache verification already exists
(`resource_manifest::verify_cache`). Resource announcements already carry file
metadata (size, SHA-256) and optional signature fields.

Download and repair logic must never execute resources. All content is
verified before it enters the local cache, and the cache is never treated as
executable.

## Current State

- `ResourceAnnouncement` contains file list with relative paths, sizes, and
  SHA-256 hashes.
- `ClientMessage::ResourceAvailabilityReport` tells the server which files the
  client already has and which are valid/invalid.
- `build_pack_index` indexes a local resource directory, rejects symlinks and
  unsafe paths.
- `verify_cache` compares local cached files against announced hashes.
- No network transfer of resource files exists.
- No download request/response protocol exists.
- No staging directory exists.
- No cache repair exists.

## Threat Model

### Malicious Server
- A compromised or malicious server could send incorrect file metadata or
  malicious payloads.
- Downloaded content must be constrained to a staging directory and verified
  before it enters the local cache.
- Server-provided relative paths must not be trusted blindly — they must be
  validated against a safe allowlist pattern.

### Corrupted Cache
- Local cache files may be missing, truncated, or bit-rotten.
- `verify_cache` detects mismatches; the download protocol repairs them.

### Partial Downloads
- A transfer may be interrupted. Staging files must not be treated as
  complete. Atomic rename or checksum-verified commit prevents half-written
  files from entering the cache.

### Path Traversal
- A malicious announcement could contain relative paths with `..` or absolute
  paths pointing outside the cache directory.
- All resource file paths must be validated: reject `..`, reject absolute
  paths, reject empty components, reject names containing path separators
  after validation.

### Symlink Attacks
- A server could announce a symlink pointing to an arbitrary file path.
- The pack index builder already rejects symlinks. The download path must also
  reject symlinks at the filesystem level when writing staging files.

### Hash Mismatch
- Every downloaded file must be verified against its announced SHA-256 before
  being moved into the cache.
- On mismatch: discard staging file, log error, optionally retry.

### Replay / Stale Metadata
- A client could receive an outdated `ResourceAnnouncement` and attempt to
  download files that no longer match the server's current index.
- Downloads should be scoped to the most recent announcement received in the
  current session.

### Unsigned Announcements
- Current announcement signatures are stub-only. An attacker could forge an
  announcement and trick the client into downloading arbitrary files.
- Until signature verification exists, the client should only download
  resources from servers it explicitly trusts (or not download at all in
  dry-run mode).

### Archive Risks
- If future milestones introduce zip/tar archives, each archive must be
  unpacked into a temporary directory and every extracted file must pass the
  same path/symlink/hash validation as individually downloaded files.
- Archive bombs (zip bombs, deep nesting, symlink escapes) must be rejected.

## Allowed Future Behaviour

1. Client may request missing or invalid files from the server after the
   resource policy evaluation allows it (dry-run or enforced).
2. Client downloads files only into a **staging directory** outside the live
   cache.
3. Client verifies file size and SHA-256 against the announcement before
   moving the file into the cache directory.
4. Client rejects symlinks, absolute paths, `..`, and unsafe file names at
   every stage (indexing, staging, cache commit).
5. Client never executes downloaded content in the download milestone.
   Execution requires a separate sandbox milestone with its own security
   review.
6. Cache repair must be explicit (user-initiated or config-gated) and
   report-only first (log what would be downloaded, do not transfer).

## Disallowed Behaviour

- No auto-execution after download.
- No writes to system directories outside the configured cache.
- No overwriting files outside the configured cache directory.
- No following symlinks during staging or cache write.
- No trusting server-provided paths without client-side validation.
- No unsigned announcement enforcement until signature verification exists.
- No GTA V integration.
- No DRM, anti-cheat, or platform security bypass.
- No Lua, JS, WASM, or scripting runtime execution.

## Proposed Future Protocol Messages

These message types are **design-only** and **not implemented**. They would
live in `crates/protocol/src/lib.rs` as new `ClientMessage` and
`ServerMessage` variants.

### Client → Server

```
ResourceDownloadRequest {
    resource_name: String,
    missing_files: Vec<String>,     // relative paths from announcement
}
```

Sent when the client's `ResourceAvailabilityReport` indicates missing or
invalid files and the join gate outcome would allow download. Lists the
specific files the client needs.

### Server → Client

```
ResourceDownloadOffer {
    resource_name: String,
    file_count: u32,
    total_size_bytes: u64,
    chunk_size: u32,               // proposed chunk size
}
```

Sent in response to a `ResourceDownloadRequest`. Confirms the server is
willing to transfer the requested files and proposes transfer parameters.

### Server → Client

```
ResourceFileChunk {
    resource_name: String,
    relative_path: String,         // which file this chunk belongs to
    offset: u64,                   // byte offset in the file
    data: Vec<u8>,                 // chunk payload (max chunk_size bytes)
    is_final: bool,                // true if this is the last chunk
}
```

Carries a chunk of a single resource file. The client reassembles the file in
the staging directory. The last chunk (`is_final: true`) signals the client
should verify the complete file.

### Client → Server (after each file)

```
ResourceDownloadComplete {
    resource_name: String,
    relative_path: String,
    size_bytes: u64,
    sha256: [u8; 32],
}
```

Sent after the client has received all chunks for a file and verified its
SHA-256 matches the announcement. The server uses this to track transfer
progress.

### Either Direction

```
ResourceDownloadError {
    resource_name: String,
    relative_path: Option<String>,  // None if error is per-resource
    error_code: String,             // e.g. "hash_mismatch", "transfer_timeout"
    message: String,
}
```

Sent when a download operation fails. Client-side errors include hash
mismatch, disk full, path validation failure. Server-side errors include
resource not found, transfer timeout.

## Staging / Cache Model

### Staging Directory

A temporary directory outside the live cache, at a configurable path or
derived from the cache directory with a `.staging` suffix:

```
cache_dir/
  resource_a/
    file1.data
    file2.data
  resource_a.staging/        ← staging during download
    file1.data.partial
    file2.data.partial
```

### Temp File Naming

- Downloading files use a `.partial` extension to distinguish from committed
  files.
- If a download is interrupted, `.partial` files are cleaned up on next
  startup or download start.

### Verify-Before-Commit

1. Receive all chunks for a file.
2. Assemble in staging directory at the correct relative path.
3. Verify total size matches announced size.
4. Compute SHA-256 of staged file.
5. Compare against announced hash.
6. If match: rename (atomic if possible) from staging to cache.
7. If mismatch: delete staging file, log error, optionally retry.

### Atomic Move

- `std::fs::rename` is atomic on the same filesystem (POSIX).
- Staging and cache directories should be on the same filesystem for atomic
  rename.
- Cross-filesystem staging requires a copy + delete fallback (non-atomic,
  with hash re-verify).

### Cleanup on Failure

- If a download is cancelled or fails mid-transfer, all `.partial` files in
  the staging directory are deleted.
- Partial commits (where some files completed before failure) remain in the
  cache — they are valid for those files.
- On server disconnect, any in-progress download is abandoned and partial
  files cleaned up.

### Deterministic Cache Layout

The cache mirrors the resource announcement structure:

```
cache_dir/
  <resource_name>/
    <relative_path_from_announcement>
```

No extra nesting, no opaque blob storage. Each resource has its own
subdirectory. The pack index builder already enforces this layout.

## Signature Relationship

- Future download integrity should rely on signed resource announcements.
- The client must verify the announcement signature before accepting any file
  metadata from the server.
- Until signature verification exists:
  - The download protocol remains in design-only status.
  - No download operations are performed in production.
  - Test-only downloads may use a known-trusted local server with
    announcements signed by a pinned test key.
- Signature enforcement is gated behind its own milestone (M3.5 candidate)
  and must be explicitly enabled via config.

## Join Gate Relationship

- The join gate (`JoinGateDecision`) should remain dry-run until the
  download, cache repair, and signature model is mature.
- Future enforcement flow:
  1. Client receives `ResourceAnnouncement`.
  2. Client builds `ResourceAvailabilityReport` from local cache.
  3. Server evaluates policy → join gate decision.
  4. If join gate would allow and enforcement is enabled, client proceeds
     to download missing files.
  5. After download completes and cache is verified, client enters the
     joined state (future milestone).
- The gate must not transition from dry-run to enforced until:
  - Download protocol is implemented and tested.
  - Cache repair is deterministic and safe.
  - Signature verification exists or a trust-on-first-use model is
    explicitly reviewed.

## Open Questions

| Question | Options | Notes |
|---|---|---|
| Chunk size | 64 KiB, 256 KiB, 1 MiB | Larger chunks reduce message overhead; smaller chunks improve partial-retry. Should be configurable or negotiated. |
| Compression | gzip, zstd, none | Compression reduces bandwidth but adds CPU cost. Ideally per-file or per-chunk opt-in. |
| Archive support | per-file only, tar, zip | Per-file is simplest and safest. Archives add extraction complexity and security surface. |
| Retry policy | none, N times, exponential backoff | Simple: no retry. Advanced: configurable retry with backoff. |
| Signed index format | inline signature in announcement, detached index file | Current stub uses inline `signature` field. Detached index allows offline signing. |
| Per-server trust roots | pinned key, CA bundle, TOFU | Determines how clients decide which servers to trust for downloads. |
| Cache eviction | LRU, age-based, never | Needed if cache grows unbounded. For early milestones, "never evict" is simplest. |
| Offline mode | skip download, use partial cache | Client may join with whatever cache it has; server decides whether to allow. |

## Next Recommended Milestones

- **M3.4**: Local resource cache repair plan — design a cache-repair flow that
  uses only local data. No network. No downloads.
- **M3.5**: Real signature verification design — design or implement
  announcement signature verification for resource authenticity.
- **M3.6**: Download protocol DTOs only — add `ResourceDownloadRequest`,
  `ResourceDownloadOffer`, `ResourceFileChunk`, `ResourceDownloadComplete`,
  `ResourceDownloadError` message types to the protocol crate. No transfer
  logic.
- **M3.7**: Staging directory model — implement staging directory creation,
  cleanup, and deterministic cache layout. Local-only. No downloads.

## Hard Boundaries

This design does not and will not:

- Implement file downloads (until explicitly milestone-gated).
- Implement file serving (server-side resource transfer).
- Execute downloaded content.
- Write outside the configured cache directory.
- Follow symlinks during any file operation.
- Trust server-provided paths without client-side validation.
- Enable join gate enforcement until signature verification exists.
- Bypass DRM, anti-cheat, or platform security.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.
