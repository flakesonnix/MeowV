# Signature Verification Engine

## Purpose

Real Ed25519 cryptographic verification engine. Consumes M3.7 verification plans and produces actual verification results. No enforcement — report-only.

## Dependency Choices

- **`ed25519-dalek` v2.2** — well-audited, widely-used Ed25519 implementation. Single algorithm keeps the blast radius small.
- **`base64` v0.22** — decode signature from wire format.
- No crypto abstraction zoo, no TLS reimplementation, no additional algorithms.

## Types

### `TrustedPublicKey`

```rust
pub struct TrustedPublicKey {
    pub key_id: String,
    pub algorithm: String,
    pub public_key: Vec<u8>,  // raw 32-byte Ed25519 public key
}
```

Like `TrustedKey` (M3.7) but carries actual key material for verification.

### `VerificationError`

```rust
pub enum VerificationError {
    UnsupportedAlgorithm(String),   // algorithm not recognized
    UnknownKeyId(String),           // key_id not in trusted set
    InvalidSignature,               // cryptographic verification failed
    MalformedKeyMaterial(String),   // public key wrong length or corrupt
    MalformedSignatureBytes(String),// signature base64 decode or length failure
    SignedPayloadMismatch(String),  // payload changed after signing (reserved)
    CanonicalPayloadMissing,        // cannot build canonical payload
}
```

All variants implement `Display`.

### `VerificationOutcome`

```rust
pub enum VerificationOutcome {
    Valid,                        // signature cryptographically verified
    Invalid { error: VerificationError },
    Skipped { reason: String },   // no signature on announcement
}
```

### `VerificationEntry`

```rust
pub struct VerificationEntry {
    pub resource_name: String,
    pub outcome: VerificationOutcome,
}
```

### `VerificationReport`

```rust
pub struct VerificationReport {
    pub entries: Vec<VerificationEntry>,
}
```

Methods:
- `is_empty() -> bool`
- `all_valid() -> bool` — true when every entry is `Valid`
- `to_text() -> String` — deterministic multi-line output

Example output:
```
signature verification report:
  [valid] chat
  total: 1 resource(s), all valid: true
```

## Core Functions

### `verify_ed25519_signature`

```rust
pub fn verify_ed25519_signature(
    payload_bytes: &[u8],       // canonical JSON payload bytes
    signature_b64: &str,         // base64-encoded Ed25519 signature
    public_key_bytes: &[u8],    // raw 32-byte public key
) -> Result<(), VerificationError>
```

Steps:
1. Base64-decode signature → 64 bytes
2. Parse 32-byte public key → `VerifyingKey`
3. Parse 64-byte signature → `Signature`
4. `verifying_key.verify(payload_bytes, &signature)`

### `execute_verification_plan`

```rust
pub fn execute_verification_plan(
    announcement: &ResourceAnnouncement,
    plan: &SignatureVerificationPlan,       // from M3.7
    trusted_keys: &[TrustedPublicKey],
) -> VerificationReport
```

Evaluates the announcement-level signature once, then maps every plan entry to a report entry with the same outcome:

| Condition | Outcome |
|-----------|---------|
| No signature on announcement | `Skipped { reason: "announcement has no signature" }` |
| Metadata validation fails (unknown alg) | `Invalid { UnsupportedAlgorithm }` |
| Metadata validation fails (empty field) | `Invalid { MalformedSignatureBytes }` |
| key_id not in trusted set | `Invalid { UnknownKeyId }` |
| Canonical payload build fails | `Invalid { CanonicalPayloadMissing }` |
| Signature verification fails | `Invalid { InvalidSignature }` |
| All checks pass | `Valid` |

## Test Coverage (14 tests)

| Test | Scenario |
|------|----------|
| `valid_signature_roundtrip` | Generate key, sign payload, verify → Valid |
| `wrong_key_fails` | Sign with key A, verify with key B → InvalidSignature |
| `corrupted_payload_fails` | Sign payload, corrupt version, verify → Invalid |
| `unsupported_algorithm_in_engine` | "rsa" algorithm → UnsupportedAlgorithm |
| `unknown_key_id_in_engine` | Valid sig, but key_id not in TrustedPublicKey set → UnknownKeyId |
| `skipped_entries_not_verified` | No signature → Skipped |
| `plan_mirroring_multi_resource` | 2 resources, both verified, sorted by name |
| `report_to_text_valid` | to_text() output for valid signature |
| `report_to_text_invalid` | to_text() output for skipped/invalid |
| `report_is_empty` | No resources → empty report |
| `malformed_signature_bytes` | Invalid base64 → MalformedSignatureBytes |
| `malformed_key_material_wrong_length` | 31-byte public key → MalformedKeyMaterial |
| `verification_error_display` | All 7 error variants Display correctly |
| `empty_plan_skipped` | No trusted keys → Skipped |

## Boundaries

- **No enforcement** — `Valid` is a verification result, no behavior change
- **No disconnects** — engine produces reports only
- **No downloads** — canonical payload is built from in-memory data
- **No cache writes**
- **No execution**
- **Ed25519 only** — no plans to add RSA, ECDSA, etc.
