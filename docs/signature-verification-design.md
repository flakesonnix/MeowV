# Signature Verification Design

## 1. Scope

This document is a **future-only design specification**. No cryptographic
verification, trust store, key management, or signature enforcement is
implemented in the current milestone. The existing
`ResourceAnnouncementSignature` struct and `check_announcement_signature_stub`
function are metadata-only and non-enforcing.

### Current State

- `ResourceAnnouncement` carries an optional `signature` field of type
  `Option<ResourceAnnouncementSignature>`.
- `ResourceAnnouncementSignature` contains three string fields:
  `algorithm`, `key_id`, `signature`.
- `check_announcement_signature_stub` returns:
  - `NotProvided` when no signature is present.
  - `NotChecked` when a signature is present.
- No actual cryptographic verification is performed.
- No trust store, key database, or public key infrastructure exists.
- No signature enforcement occurs at any point in the handshake.
- `SignatureVerificationStatus` enum already defines `Valid`, `Invalid`,
  `UnsupportedAlgorithm`, `NotProvided`, `NotChecked` — these are ready for
  future use but currently only `NotProvided` and `NotChecked` are returned.

### Current Behaviour Summary

| Condition | Status | Flow Continues? |
|---|---|---|
| No signature | `NotProvided` | Yes |
| Signature present (any algorithm) | `NotChecked` | Yes |
| Signature present (Ed25519, future) | `NotChecked` | Yes |
| Signature invalid (future) | `NotChecked` | Yes |

No enforcement happens until a future milestone explicitly activates it.

## 2. What Should Be Signed

The signature should cover the entire set of metadata that the client needs to
trust before accepting resource files from the server. This includes:

### Fields to Sign

| Field | Type | Why |
|---|---|---|
| `protocol_version` | `u32` | Prevents cross-version replay |
| `resource_name` | `String` | Binds signature to a specific resource |
| `resource_version` | `String` | Binds signature to a specific version |
| `requirement_level` | enum | Prevents downgrade of requirement |
| `files` | `Vec<FileEntry>` | The core payload — paths, sizes, hashes |

Each `FileEntry` in the signed payload must include:
- `relative_path` — deterministic, normalized, no `..`, no absolute
- `size_bytes` — `u64`
- `sha256` — hex-encoded lowercase SHA-256 string

### Fields Optionally Included

| Field | Rationale |
|---|---|
| `edition_compatibility` | Binds to a specific game edition if desired |
| `platform_compatibility` | Binds to a specific platform if desired |
| `tags` | Optional metadata, low security value |
| `dependencies` | Resource dependency list, high value if signed |
| `timestamp` / `validity_window` | Adds freshness — see open questions |

### Fields NOT to Sign (outside payload)

- `algorithm` — metadata about how the signature was created; must be readable
  before verification
- `key_id` — identifies which key to use; must be readable before verification
- `signature` — the cryptographic signature itself

### Tradeoff: Key Metadata Inside or Outside Signed Payload

| Approach | Pros | Cons |
|---|---|---|
| `algorithm` and `key_id` outside | Parser can select verifier before extracting payload | Tampered `algorithm`/`key_id` not detected unless also covered |
| `algorithm` and `key_id` inside | Tamper-proof binding of algorithm and key identity | Requires parser to try candidate algorithms before knowing which one to use |

**Recommendation**: Include `algorithm` and `key_id` inside the signed payload
AND allow them to be read from the envelope for dispatch. Verification must
check that the envelope values match the signed values. This gives both
dispatch convenience and tamper evidence.

## 3. Canonicalization

Signature verification requires that both signer and verifier produce the
identical byte string to verify. Deterministic serialization is critical.

### Requirements

1. **Stable field order**: Fields must be serialized in a fixed, documented
   order (e.g., alphabetical by field name, or as defined in a canonical schema).
2. **Stable resource order**: When signing multiple resources (e.g., an entire
   announcement), resources must be sorted deterministically (e.g., by name).
3. **Stable file order**: Files within each resource must be sorted
   deterministically (e.g., by `relative_path`).
4. **UTF-8 encoded**: All string values must be UTF-8. No BOM.
5. **Normalized relative paths**: Forward slashes only, no leading `./`, no
   trailing slash, no empty segments, no `.` or `..` components.
6. **No absolute paths**: Rejected at the pack index level.
7. **No `..` traversal**: Rejected at the pack index level.
8. **No symlinks**: Rejected at the pack index level.
9. **No non-deterministic maps**: Use ordered maps (e.g., `BTreeMap`) or
   sorted vectors instead of `HashMap` for any keyed data.
10. **No timestamps unless explicitly included**: If timestamps are not part of
    the signed payload, they must not appear in the canonical form at all. If
    they are added, they must be mandatory and verified.
11. **No optional fields with default values**: Either include every field
    explicitly or define a canonical encoding that omits them deterministically.

### Proposed Canonical Encoding

Use the existing serde JSON serialization with `#[serde(rename_all =
"snake_case")]` and a deterministic field order maintained by the struct
definition. A future milestone may switch to a more constrained format (see
open questions), but starting with JSON provides:

- Easy debugging and inspection
- Existing serde infrastructure
- Deterministic output when struct field order is fixed and maps are avoided

### Signing Input Construction

```
1. Serialize the announcement payload (without signature envelope) to JSON.
2. Ensure deterministic field ordering via struct definition.
3. Ensure deterministic resource ordering (sorted by name).
4. Ensure deterministic file ordering (sorted by relative_path).
5. UTF-8 encode the JSON string.
6. Sign the resulting byte string.
```

## 4. Proposed Algorithms

### Primary Recommendation: Ed25519

| Property | Ed25519 |
|---|---|
| Signature size | 64 bytes (encoded ~88 base64) |
| Public key size | 32 bytes (encoded ~44 base64) |
| Speed | Fast verification, fast signing |
| Security | High (128-bit security level) |
| Implementation | `ed25519-dalek` (Rust), widely audited |
| Deterministic | Yes (RFC 8032) |

### Why Ed25519

- Small signatures and keys — efficient for in-protocol messages.
- Fast verification — important for client-side startup performance.
- Deterministic — no nonce concerns, no signature malleability.
- Widely deployed in modern cryptographic systems.
- No patent concerns.

### Algorithm Agility

The `algorithm` field already exists in `ResourceAnnouncementSignature`. When
verification is implemented:

- `algorithm` must be checked against a known set (initially just `"ed25519"`).
- Unknown algorithms produce `UnsupportedAlgorithm` status.
- Algorithm agility allows future migration (e.g., to post-quantum signatures)
  without changing the protocol shape.

### Do Not Invent Custom Crypto

- No custom hash-then-sign constructions.
- No ad-hoc MAC-based schemes.
- Use standard Ed25519 as specified in RFC 8032.
- Use SHA-256 (already in use for file hashing) for any hashing needs outside
  the signature algorithm itself.

### Rejected Alternatives

| Algorithm | Reason |
|---|---|
| RSA-2048/4096 | Larger signatures and keys, slower verification |
| ECDSA (P-256) | Non-deterministic without careful handling, more complex |
| Schnorr | Less library support, no clear advantage over Ed25519 |
| BLS | Pairing-based, more complex, overkill for this use case |
| HMAC-SHA256 | Symmetric — requires shared secret, not suitable for per-server trust |
| Raw SHA-256 | No authentication — anyone can produce a valid hash |

## 5. Trust Model

### Per-Server Trust Root

Each server the client connects to has its own trust root. There is no global
certificate authority. Trust is established either by:

- **Pinned public key**: The client knows the server's Ed25519 public key ahead
  of time (e.g., from a config file, server list metadata, or out-of-band
  exchange).
- **First-use trust**: The client accepts the server's key on first connection
  and remembers it (TOFU — Trust On First Use).
- **Server browser metadata**: The server list may include the server's public
  key fingerprint as an additional trust hint.

### Pinned Public Keys

Future config (server-side or client-side) could specify trusted keys:

```toml
[trust]
# Client-side example (future)
server_keys = [
    { server_id = "my-server", public_key = "MCowBQYDK2VwAyEA..." },
]
```

Key pinning is the most secure option but requires an out-of-band key exchange.

### key_id Design

`key_id` is an opaque string that identifies which public key should be used to
verify the signature. Key IDs:

- Must be unique within a server's key set.
- Should be stable across key rotations (or rotated with a clear mapping).
- Could be a hash of the public key (self-certifying), or a human-readable
  label chosen by the server operator.

### Key Rotation

When a server operator rotates keys:

1. New announcements are signed with the new key.
2. `key_id` changes to identify the new key.
3. Old announcements signed with the old key remain verifiable if the client
   retains the old public key (for the duration of the session).
4. Clients that only know the old key will fail to verify — this is expected
   and should produce a clear error message.

### Revoked Keys

Future key revocation could be handled by:

- A server-side revocation list published as part of the announcement or a
  separate metadata channel.
- Client-side revocation list maintained in local config.
- Short-lived key validity windows to limit the damage of a compromised key.

No revocation mechanism is designed or implemented in this milestone.

### First-Use Trust Risks

TOFU (Trust On First Use) is vulnerable to:

- **Impersonation on first connection**: An attacker who intercepts the first
  connection can present their own key and be trusted thereafter.
- **Key mismatch on reconnect**: If the server's key changes between
  connections (legitimate rotation or attack), TOFU clients will see a
  mismatch and must decide whether to accept the new key.

These risks are acceptable for a development-stage prototype. Production
deployments should use pinned keys.

### Local Config for Trusted Keys

A future client config section could specify trusted public keys:

```toml
[trust]
# Path to a directory of trusted public keys (PEM or raw base64)
key_dir = "/etc/meowv/trusted-keys"

# Alternatively, inline keys
[[trust.keys]]
key_id = "server-main"
public_key = "MCowBQYDK2VwAyEA..."
```

This config would be local-only, never transmitted, never stored in the
registry or diagnostics.

### No Global Trust Assumptions

- No built-in certificate authorities.
- No vendored public keys.
- No hard-coded trust anchors.
- Trust is always explicitly configured or explicitly accepted by the user.

## 6. Verification Flow

### Future Verification Flow (design only)

```
Client receives ResourceAnnouncement
  │
  ├─ Check if signature is present
  │    ├─ No signature → status = NotProvided
  │    │                   ↓ (if enforcement: reject)
  │    └─ Signature present → continue
  │
  ├─ Read algorithm and key_id from envelope
  │    ├─ algorithm unknown → status = UnsupportedAlgorithm
  │    │                      ↓ (if enforcement: reject)
  │    └─ algorithm known → continue
  │
  ├─ Look up public key by key_id
  │    ├─ key_id not found → status = Invalid (unknown key)
  │    │                      ↓ (if enforcement: reject)
  │    └─ key found → continue
  │
  ├─ Canonicalize announcement payload (without signature envelope)
  │
  ├─ Verify signature bytes against canonical payload + public key
  │    ├─ Invalid → status = Invalid
  │    │             ↓ (if enforcement: reject)
  │    └─ Valid → status = Valid
  │
  └─ Continue to resource availability checking and join gate
```

### Integration with Current Handshake

The current handshake flow calls `check_announcement_signature_stub` after
receiving the `ResourceAnnouncement`. A future implementation would replace
the stub with real verification, but the integration point remains the same.

### Relationship to Cache Verification

Signature verification and cache verification are independent:

1. Signature verification confirms the announcement came from the expected
   authority and was not tampered with.
2. Cache verification (SHA-256) confirms each cached file matches the
   announced hash.
3. Both must pass before the client can trust that it has the correct files.
4. Signature verification must happen first — you cannot trust the file hashes
   until you trust the announcement.

### Downloads Should Require Verified Announcements

Until signature verification exists:

- Downloads must not be performed (they are not implemented anyway).
- The download protocol remains in design-only status.
- Once signature verification is implemented and optionally enforced, downloads
  should require a `Valid` signature before any file transfer begins.

## 7. Failure Modes

| Failure Mode | Detection | Current Response | Future Enforced Response |
|---|---|---|---|
| **No signature** | `signature` is `None` | `NotProvided`, flow continues | Reject announcement, log error |
| **Unsupported algorithm** | `algorithm` not in known set | `NotChecked`, flow continues | `UnsupportedAlgorithm`, reject |
| **Unknown key_id** | `key_id` not in trust store | `NotChecked`, flow continues | `Invalid`, reject |
| **Invalid signature** | Verification fails | `NotChecked`, flow continues | `Invalid`, reject |
| **Expired/stale metadata** | `timestamp` outside window (future) | N/A | Reject, request fresh announcement |
| **Replayed old signed index** | Version mismatch or timestamp check | Not detected | Reject if version/timestamp outside window |
| **Canonicalization mismatch** | Signer and verifier produce different bytes | Not detected | Signature fails, `Invalid` |
| **Partial files** | Extra or missing files in announcement | Not detected by signature | Signature covers file list — mismatch detected |
| **Malicious server with trusted key** | Key is compromised | Not detected | Key revocation or out-of-band trust update |

### Notes on Specific Failure Modes

**Replayed Old Signed Index**: A valid signature proves the announcement was
signed by the trusted key, but does not prove it is current. The server could
send a week-old valid announcement. Mitigations:

- Include a `timestamp` or `sequence_number` in the signed payload.
- Client rejects announcements whose timestamp is too old or too far in the
  future.
- Client tracks the highest sequence number seen per server and rejects older
  ones.

**Canonicalization Mismatch**: The most likely source of verification bugs. If
the signer (server) and verifier (client) serialize the announcement
differently, valid signatures will fail. Mitigations:

- Use the same serialization library on both sides.
- Document the canonical form explicitly.
- Test that signing and verifying produce identical byte strings.

**Malicious Server with Trusted Key**: Signature verification only proves that
the announcement was created by whoever holds the private key. If that key is
compromised, signature verification provides no protection. Mitigations:

- Key rotation limits the window of compromise.
- Revocation mechanisms allow clients to reject known-compromised keys.
- Pinning a specific key (rather than accepting any key with a given key_id)
  reduces blast radius.

## 8. Enforcement Policy

### Current (Milestone 3.5)

- Signatures remain **non-enforcing**.
- `check_announcement_signature_stub` continues to return `NotProvided` /
  `NotChecked`.
- Flow continues unchanged regardless of signature status.

### Future Enforcement Phases

Each phase must be explicitly gated by a milestone, a config flag, or both.

| Phase | Milestone | Config Gate | Behaviour |
|---|---|---|---|
| Report-only verification | M3.7 | `signature_verification = "report"` | Verify but never reject; log results |
| Optional signatures | Future | `signature_requirement = "optional"` | Reject if signature present but invalid; allow unsigned |
| Required signatures | Future | `signature_requirement = "required"` | Reject unsigned or invalid announcements |
| Required before downloads | Future | (implicit with download enable) | Do not download unless signature is `Valid` |
| Required before join gate | Future | (implicit with join gate enable) | Do not allow join unless signature is `Valid` |

### Interaction with Protocol Version

- Exact protocol version matching remains enforced throughout all phases.
- Protocol negotiation (dry-run) stays dry-run until explicitly activated.
- Signature enforcement is independent of protocol version — a signed
  announcement with a mismatched protocol version would still be rejected by
  the version check regardless of signature status.

### Config Validation

When signature features are added to config, the validator must reject
dangerous combinations:

- `signature_requirement = "required"` + `enforce_required_resources = true`
  without signed announcements being fully tested.
- `signature_requirement = "optional"` + download protocol enabled (unsigned
  downloads would be accepted).

## 9. Relationship to Resource Download Design

### Separation of Concerns

| Layer | What It Verifies | When |
|---|---|---|
| Signature | Announcement authenticity and integrity | Before accepting file metadata |
| Hash (SHA-256) | Individual file content integrity | After download, before cache commit |
| Path validation | File path safety | Before staging write, before cache commit |
| No-exec boundary | Content is not executed | Always — separate milestone |

### Downloads Must Not Execute Content

Even with a valid signature and verified hashes, downloaded files must never
be executed. Signature verification does not make scripts safe to run. A
separate sandbox milestone is required before any execution.

### Staging and Verify-Before-Commit Still Required

Signature verification of the announcement does not eliminate the need for:

- Staging directory for downloads (separate from cache).
- `.partial` file naming during transfer.
- SHA-256 verification before commit to cache.
- Atomic rename on commit.
- Cleanup on failure.

The download staging model (described in `docs/resource-download-design.md`)
remains unchanged regardless of signature status.

### Signature Does Not Make Scripts Safe to Run

- A valid signature proves the metadata came from the expected server.
- It does not prove the file contents are safe to execute.
- It does not replace sandboxing, permissions, or code review.
- Execution requires a separate sandbox milestone with its own security review.

### Hash Verifies File Content Integrity

SHA-256 hashes in the announcement provide:

- Integrity check: the file on disk matches what the server announced.
- Tamper detection: if the announcement is signed, the hashes are also
  authenticated (because they are part of the signed payload).
- Repair target: the expected hash tells the client what the file should look
  like after download.

### Mutual Dependency

- Signature verification requires the canonical announcement payload, which
  includes the file hashes.
- Download repair requires the file hashes (from the verified announcement) to
  verify downloaded content.
- Cache verification requires the file hashes to compare against local files.
- Therefore: verify signature first → trust the hashes → use the hashes for
  cache verification and download repair.

## 10. Open Questions

| # | Question | Options | Recommendation |
|---|---|---|---|
| 1 | **Signed JSON vs canonical custom format** | JSON with serde, CBOR, BCS, custom | Start with JSON for debuggability; document canonicalization rules. Switch to a stricter format (CBOR or BCS) if performance or determinism becomes an issue. |
| 2 | **Key storage format** | PEM, raw base64, SSH-style, custom TOML | Store Ed25519 keys as base64-encoded 32-byte public keys in TOML config. Use `ed25519` prefix or `algorithm` field to disambiguate. |
| 3 | **Key rotation UX** | Manual config update, key directory scan, automatic fetch | Start with manual config updates. A future milestone could support a directory of trusted keys (hot-reload on file change). |
| 4 | **Revocation mechanism** | CRL-style list, short-lived keys, online status check | Use short-lived key validity windows first. CRL-style revocation is complex and adds state. |
| 5 | **Validity timestamps** | `issued_at` + `expires_at` in signed payload | Include both. Clients reject announcements outside the validity window. Default window: 24 hours. |
| 6 | **Offline mode** | Skip signature check, use cached trust, refuse to connect | If the client cannot verify (no network for CRL, no clock), it should refuse to connect unless explicitly configured to allow offline mode. |
| 7 | **Server browser trust metadata** | Include key fingerprint in `ServerEntry`, have browser verify | Server list entries could include a `key_fingerprint` field. The browser could display mismatches but not enforce. |
| 8 | **Multi-resource bundle signatures** | Sign entire announcement, sign per-resource, both | Sign the entire announcement (one signature for all resources). Per-resource signatures add complexity with marginal benefit. |
| 9 | **Per-resource vs per-announcement signatures** | One signature for all resources in the announcement | One signature is simpler, more efficient, and prevents resource reordering attacks. If per-resource signing is needed later, add it as an extension. |
| 10 | **Signature format encoding** | base64, hex, raw bytes in message | base64 is standard for JSON-based protocols. Keep as `String` with base64 encoding. |
| 11 | **Timestamp source** | Server clock, NTP, monotonic sequence | Use server-provided `timestamp` (Unix epoch seconds). Client rejects if too far from local clock. Future: allow configurable drift tolerance. |
| 12 | **Key ID format** | Hash of public key, server-chosen label, UUID | Use SHA-256 hash of the public key (truncated to 8 bytes, hex-encoded) as the default `key_id`. This is self-certifying and avoids naming collisions. |

## 11. Next Milestones

The following milestones are recommended after M3.5 (this document). Each is
a small, focused step that preserves the no-execution and dry-run boundaries.

| MS | Title | Description |
|---|---|---|
| M3.6 | Signature verification protocol DTO refinement | No crypto. Add canonicalization helpers, refine `SignatureVerificationStatus` usage, add `signature_verification` field to announcement DTO if needed. Report-only. |
| M3.7 | Ed25519 verification implementation | Add `ed25519-dalek` dependency, implement real canonicalization + verification in the protocol crate. **Report-only** — log results, never reject. |
| M3.8 | Trusted key config | Add client-side or server-side config for trusted public keys. Local-only. No network fetch. |
| M3.9 | Download protocol DTOs only | Add `ResourceDownloadRequest`, `ResourceDownloadOffer`, `ResourceFileChunk`, `ResourceDownloadComplete`, `ResourceDownloadError` message types to the protocol crate. No transfer logic. |
| M3.10 | Staging directory model | Implement staging directory creation, cleanup, and deterministic cache layout. Local-only. No downloads. |

### Boundaries Preserved Across All Milestones

- No execution of downloaded or cached content.
- No GTA V integration.
- No remote admin API.
- No persistence or telemetry.
- No proprietary or copied implementation details.
- All new features gated by config, disabled by default.
- Exact protocol version matching remains enforced.
- Protocol negotiation remains dry-run.
- Join gate remains dry-run.
- Capability gating remains report-only.

## Related Documents

- `docs/signed-resource-announcements.md` — current stub implementation
- `docs/resource-download-design.md` — download protocol design (spec only)
- `docs/resource-cache-repair-plan.md` — cache repair planning (report-only)
- `docs/security-boundaries.md` — operational security boundaries
- `docs/architecture.md` — crate/module map, pipelines, dry-run policies
