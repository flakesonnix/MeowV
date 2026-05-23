# Signature Metadata Model

## Purpose

Typed signature metadata model and structural validation for resource announcements. No cryptographic verification — pure data validation and canonicalization.

## Types

### `SignatureAlgorithm`

```rust
pub enum SignatureAlgorithm {
    Ed25519,
}
```

- `Display` → `"ed25519"`
- `FromStr` — known algorithms parse; unknown produce `SignatureMetadataError::UnsupportedAlgorithm`
- `known_names() -> &'static [&'static str]` — returns `&["ed25519"]`
- `is_known(name: &str) -> bool` — convenience predicate

### `SignatureMetadataError`

```rust
pub enum SignatureMetadataError {
    EmptyAlgorithm,
    EmptyKeyId,
    EmptySignature,
    UnsupportedAlgorithm(String),
}
```

- All variants implement `Display` with descriptive messages.

### `validate_signature_metadata`

```rust
pub fn validate_signature_metadata(
    signature: &ResourceAnnouncementSignature,
) -> Result<(), SignatureMetadataError>
```

Validation order:
1. `algorithm` non-empty → `EmptyAlgorithm`
2. `key_id` non-empty → `EmptyKeyId`
3. `signature` non-empty → `EmptySignature`
4. Algorithm is known via `SignatureAlgorithm::from_str` → `UnsupportedAlgorithm`

## Canonical Payload

Defines what **would** be signed by a future verification step.

### `CanonicalAnnouncementPayload`

```rust
pub struct CanonicalAnnouncementPayload {
    pub protocol_version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub resources: Vec<CanonicalResourcePayload>,
}
```

### `CanonicalResourcePayload`

```rust
pub struct CanonicalResourcePayload {
    pub name: String,
    pub version: String,
    pub requirement_level: ResourceRequirementLevel,
    pub files: Vec<CanonicalFilePayload>,
}
```

### `CanonicalFilePayload`

```rust
pub struct CanonicalFilePayload {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}
```

### `build_canonical_payload`

```rust
pub fn build_canonical_payload(
    announcement: &ResourceAnnouncement,
) -> Option<CanonicalAnnouncementPayload>
```

- Returns `None` when signature is `None` or algorithm/key_id are empty.
- Deterministic ordering:
  - Resources sorted by `name` (lexicographic ascending).
  - Files sorted by `relative_path` per resource.
- `protocol_version` taken from `resources[0].protocol_version` (all resources in one announcement share the same version).
- `algorithm` and `key_id` copied from signature metadata (included in canonical payload per design recommendation).

## Updated Stub Behaviour

`check_announcement_signature_stub` now uses `validate_signature_metadata`:

| Condition | Status | Reason |
|-----------|--------|--------|
| No signature | `NotProvided` | "resource announcement signature not provided" |
| Unsupported algorithm | `UnsupportedAlgorithm` | "unsupported signature algorithm: '...'" |
| Empty algorithm/key_id/sig | `Invalid` | descriptive message from `SignatureMetadataError` |
| All metadata valid | `NotChecked` | "cryptographic verification not enforced in this milestone" |

## Test Coverage (21 new tests)

- `validate_signature_metadata_valid_ed25519` — known algorithm passes
- `validate_signature_metadata_empty_algorithm` — empty algorithm rejected
- `validate_signature_metadata_empty_key_id` — empty key_id rejected
- `validate_signature_metadata_empty_signature` — empty signature rejected
- `validate_signature_metadata_unknown_algorithm` — unknown algorithm rejected
- `signature_algorithm_display_and_from_str_roundtrip` — Ed25519 round-trips
- `signature_algorithm_unknown_from_str_fails` — "rsa" rejected
- `signature_algorithm_known_names_includes_ed25519`
- `signature_algorithm_is_known` — known/unknown predicate
- `stub_returns_not_provided_when_no_signature`
- `stub_returns_not_checked_for_valid_metadata`
- `stub_returns_unsupported_algorithm_for_unknown_algorithm`
- `stub_returns_invalid_for_empty_algorithm`
- `stub_returns_invalid_for_empty_key_id`
- `stub_returns_invalid_for_empty_signature`
- `build_canonical_payload_none_when_no_signature`
- `build_canonical_payload_none_when_empty_algorithm`
- `build_canonical_payload_none_when_empty_key_id`
- `build_canonical_payload_deterministic_sorting` — resources/files sorted
- `build_canonical_payload_json_serialization_roundtrip`
- `signature_metadata_error_display` — Display messages contain expected text

## Boundaries

- No cryptographic verification
- No crypto dependencies
- No base64 validity check (signature is a plain string)
- No key storage, trust store, or key management
- No file I/O, network, or execution
- `SignatureAlgorithm` only knows `Ed25519`; other algorithms produce structured `UnsupportedAlgorithm` error
