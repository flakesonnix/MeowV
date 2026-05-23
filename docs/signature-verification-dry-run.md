# Signature Verification Dry-Run Planner

## Purpose

Deterministic, report-only planning layer that decides what would need to be verified and what would be rejected — using the M3.6 signature metadata model. No crypto, no enforcement.

## Types

### `TrustedKey`

```rust
pub struct TrustedKey {
    pub key_id: String,
    pub algorithm: String,
}
```

Lightweight key identity for dry-run planning. No actual crypto key material. The planner checks if a signature's `algorithm` + `key_id` match any trusted key.

### `SignatureVerificationAction`

```rust
pub enum SignatureVerificationAction {
    VerifySignature,             // Metadata valid, algorithm known, key trusted → would verify
    MissingSignature,            // No signature on announcement
    UnsupportedAlgorithm,        // Algorithm not recognized
    UnknownKeyId,                // Key ID not in trusted set
    MalformedSignature,          // Metadata failed structural validation
    WouldRejectUnsigned,         // Policy would reject unsigned
    ResourceDigestMismatchPrecheck, // Reserved for future use
}
```

Each variant implements `Display` with a `snake_case` string (e.g. `"verify_signature"`, `"unknown_key_id"`).

### `SignatureVerificationPlanEntry`

```rust
pub struct SignatureVerificationPlanEntry {
    pub resource_name: String,
    pub action: SignatureVerificationAction,
    pub reason: String,
}
```

One entry per resource in the announcement. The `action` is determined by the announcement-level signature and trust policy.

### `SignatureVerificationPlan`

```rust
pub struct SignatureVerificationPlan {
    pub entries: Vec<SignatureVerificationPlanEntry>,
}
```

Methods:
- `is_empty() -> bool` — true when no resources
- `to_text() -> String` — deterministic multi-line output, entries sorted by resource name

Example output:
```
signature verification plan:
  [verify_signature] chat — algorithm 'ed25519', key 'dev-key': trusted, would verify
  total: 1 resource(s)
```

## Builder Function

```rust
pub fn build_signature_verification_plan(
    announcement: &ResourceAnnouncement,
    trusted_keys: &[TrustedKey],
    reject_unsigned: bool,
) -> SignatureVerificationPlan
```

Pure function, no I/O, no crypto. Decision logic per resource:

| Condition | Action |
|-----------|--------|
| No signature, `reject_unsigned=false` | `MissingSignature` |
| No signature, `reject_unsigned=true` | `WouldRejectUnsigned` |
| Metadata validation fails (empty field) | `MalformedSignature` |
| Metadata validation fails (unknown alg) | `UnsupportedAlgorithm` |
| Metadata valid, key_id not trusted | `UnknownKeyId` |
| Metadata valid, key_id trusted | `VerifySignature` |

Results are sorted by `resource_name` for deterministic output.

## Test Coverage (16 new tests)

| Test | Scenario |
|------|----------|
| `plan_valid_signed_announcement_trusted_key` | Valid ed25519 sig + matching trusted key → VerifySignature |
| `plan_no_signature_no_reject_unsigned` | No sig, reject=false → MissingSignature |
| `plan_no_signature_reject_unsigned` | No sig, reject=true → WouldRejectUnsigned |
| `plan_unsupported_algorithm` | "rsa" algorithm → UnsupportedAlgorithm |
| `plan_unknown_key_id` | Valid sig but key not in trusted set → UnknownKeyId |
| `plan_malformed_signature_empty_algorithm` | Empty algorithm field → MalformedSignature |
| `plan_malformed_signature_empty_key_id` | Empty key_id → MalformedSignature |
| `plan_malformed_signature_empty_sig` | Empty signature → MalformedSignature |
| `plan_multiple_resources_sorted` | 3 resources, all verified, sorted by name |
| `plan_to_text_output_format` | to_text() contains expected markers |
| `plan_to_text_empty` | Empty resources → "(empty, no resources)" |
| `plan_action_display` | All 7 action variants Display correctly |
| `plan_key_trusted_any_algorithm_match` | algorithm mismatch → not trusted |
| `plan_no_trusted_keys_all_unknown` | Empty trusted set → all UnknownKeyId |
| `plan_trusted_key_matches_case_sensitive` | Case mismatch → UnknownKeyId |
| `plan_empty_resources` | No resources → plan is empty |

## Boundaries

- No cryptographic verification — `VerifySignature` is a dry-run action, not actual verification
- No crypto dependencies
- No downloads, cache writes, resource execution
- No enforcement or disconnect behavior
- `ResourceDigestMismatchPrecheck` reserved — not returned by current logic
- `TrustedKey` holds identity only, no key material
