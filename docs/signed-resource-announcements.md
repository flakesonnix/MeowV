# Signed Resource Announcements

## Purpose

Current milestone adds signature metadata and a non-enforcing signature stub
for resource announcements. This prepares the protocol for future trust
validation without enforcing signatures yet.

Detailed design specification for real signature verification is in
`docs/signature-verification-design.md`.

## Current Scope

- metadata only
- no real signature verification
- no trust store
- no enforcement
- no downloads
- no execution
- no GTA integration

## What Exists

### `ResourceAnnouncementSignature`

```rust
pub struct ResourceAnnouncementSignature {
    pub algorithm: String,  // e.g. "ed25519"
    pub key_id: String,     // identifies the signing key
    pub signature: String,  // the cryptographic signature (base64)
}
```

This struct is carried as an optional field on `ResourceAnnouncement`:

```rust
pub struct ResourceAnnouncement {
    pub resources: Vec<AnnouncedResource>,
    pub signature: Option<ResourceAnnouncementSignature>,
}
```

### `SignatureVerificationStatus`

```rust
pub enum SignatureVerificationStatus {
    NotProvided,
    UnsupportedAlgorithm,
    Invalid,
    Valid,
    NotChecked,
}
```

All five variants exist but only `NotProvided` and `NotChecked` are currently
returned.

### `check_announcement_signature_stub`

A non-enforcing stub that:

- Returns `NotProvided` when no signature is present.
- Returns `NotChecked` when a signature is present (regardless of algorithm,
  key_id, or signature value).
- Does not verify any cryptographic data.
- Does not block or alter the handshake flow.

## Current Behaviour

| Condition | Status | Flow Continues? |
|---|---|---|
| No signature | `NotProvided` | Yes |
| Signature present (any) | `NotChecked` | Yes |
| Signature invalid | `NotChecked` | Yes |
| Unknown algorithm | `NotChecked` | Yes |

## Future Design

See `docs/signature-verification-design.md` for comprehensive design
specification covering:

- What should be signed (canonical payload definition)
- Canonicalization requirements (stable field order, UTF-8, no maps)
- Proposed algorithm (Ed25519)
- Trust model (per-server pinned keys, TOFU, key rotation)
- Verification flow (integration with current handshake)
- Failure modes (no signature, invalid, replay, canonicalization mismatch)
- Enforcement policy (report-only first, phased activation)
- Relationship to download design and cache verification
- Open questions (key format, revocation, timestamps, multi-resource signing)
- Next milestones (DTO refinement, Ed25519 impl, trusted key config)

## Not Implemented Yet

- real cryptographic verification — designed in `docs/signature-verification-design.md`
- trusted key distribution — future milestone
- signature enforcement — future milestone
- signed download pipeline — future milestone

## Clean-Room Note

Signature and trust design must remain original. Do not copy proprietary trust
bootstrapping, signature packaging, or launcher verification flows from GTA V
multiplayer ecosystems. See `docs/signature-verification-design.md` for the
clean-room design specification.
