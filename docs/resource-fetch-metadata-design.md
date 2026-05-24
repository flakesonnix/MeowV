# Resource Fetch Metadata Design (M6.3)

Purpose

- Define a future, safe, and deterministic metadata model for describing
  resource fetch sources in resource announcements. This is a design-only
  milestone: no network, no fetching, no cache writes, and no execution.

Goals

- Allow announcements to describe where to fetch announced resource files
  (HTTP(S) URLs, local references, mirrors, or other future schemes) in a
  way that is safe to parse, validate, and plan against before any bytes are
  transferred.
- Ensure metadata is authoritative only when paired with digest/signature
  material; never trust URLs alone.
- Define validation rules that prevent common attack classes (path
  traversal, ambiguous sources, unsupported schemes, missing digests).
- Keep the model extensible for future staged/sandboxed fetch implementations.

Design Scope

- This doc defines the metadata shape and validation rules. It does not
  change the wire format yet; the intent is to document a future shape that
  can be introduced in a protocol iteration or carried in an auxiliary
  resource index.

Metadata Model (proposal)

AnnouncedResourceFile will gain an optional `sources` field (design-only):

```text
AnnouncedResourceFile {
  relative_path: String,
  size_bytes: u64,
  sha256: String,
  sources: Option<Vec<ResourceFetchSource>>
}
```

ResourceFetchSource (design DTO):

```text
ResourceFetchSource {
  id: Option<String>,             // optional stable id for deterministic ordering
  scheme: String,                 // e.g. "https", "file", "ipfs" (allowed set documented)
  uri: String,                    // opaque URI to locate the file under the scheme
  size_bytes: Option<u64>,        // optional; must match announced size if present
  sha256: Option<String>,         // optional; must match announced digest if present
  compression: Option<String>,    // e.g. "gzip", "xz", "none"
  media_type: Option<String>,     // optional MIME-type for future hints
  priority: Option<u8>,           // lower value = higher priority; default 100
  mirrors: Option<Vec<String>>,   // list of alternate URIs (same semantics)
}
```

Notes

- `id` is optional but recommended for deterministic ordering and de-dup.
- `scheme` must be from a curated allowlist (see Validation rules).
- `uri` is opaque to the client — it is only parsed and validated for scheme
  and not blindly executed.
- `sha256` and `size_bytes` are authoritative when present — planners must
  prefer digest/size pairing for verification.

Validation Rules

- Allowed schemes: `https`, `http` (operator opt-in), `file` (local only,
  advisory), `ipfs` (future), and any future scheme added purposely.
- Reject any source with an unsupported scheme.
- Require at least one of `sha256` or `size_bytes` to be present across the
  file-level announcement or source-level entry. Prefer insisting on `sha256`.
- When `size_bytes`/`sha256` are provided on the source, they must match the
  announced file-level `size_bytes`/`sha256` if those exist; otherwise the
  announcement should include canonical size/digest at the file-level.
- Reject sources whose `uri` contain obvious path traversal when scheme is
  `file` or when URIs include path-like components relevant to extraction.
- Reject duplicate source entries (same scheme+uri) or ambiguous entries
  where identical URIs appear with different authoritative digests.
- Deterministic ordering: plan sorts sources by (priority asc, id asc, uri
  asc) to guarantee stable behavior across runs.

Trust / Security Model

- URLs are not trusted. Digests and signatures are authoritative.
- The presence of `sha256` and valid announcement signature (future) is the
  minimum required to treat a downloaded blob as authoritative.
- Plan-only behavior: before any fetch, the planner will produce a dry-run
  fetch plan that lists which source(s) would be attempted, in which order,
  and verification steps required (digest verification, signature checks).
- Fetch-time rules (future enforcement): a fetched file must verify the
  announced digest before atomically moving into the cache. Verification
  failures result in ReplaceInvalid semantics (design only here).
- No resource execution is permitted as part of fetch/verify flow.

Phased Workplan

1. M6.3 (this milestone): design and document ResourceFetchSource DTO,
   validation rules, deterministic ordering, and trust model.
2. M6.4: implement DTOs in protocol crates + unit tests for validation and
   deterministic plan generation (still no network). Add planner that maps
   sources→fetch-actions.
3. M6.5: implement sandboxed fetch executor behind opt-in flag, verify-only
   pipeline, and atomic cache writes with verification before commit.

Operational Safety

- This design explicitly avoids any network access, cache mutation, or
  execution. It is a specification and test-only change until M6.4+.
- Validation must be deterministic and side-effect free so that preflight
  tools remain safe to run in CI or developer machines.

Roadmap update

- Added Milestone 6.3 entry (design-only) and reference to subsequent M6.4
  as implementation of DTOs + validation.

Open questions

- How to express authenticated mirror lists (per-mirror signatures or
  combined index signatures)?
- TOFU vs pinned-trust model for servers serving resource blobs.
- Archive packaging: how to describe multi-file archives vs individual
  file fetches and path mapping inside archives safely.

References

- docs/resource-download-preflight.md
- docs/resource-download-design.md (existing design notes)
- docs/signed-resource-announcements.md
