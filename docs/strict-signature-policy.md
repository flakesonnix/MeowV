# Strict Signature Policy

## Purpose

First real behavior change in the signature verification pipeline. Under explicit opt-in, the client rejects resource announcements that fail signature verification.

## Policy

```rust
pub enum SignaturePolicy {
    ReportOnly,   // default — never rejects
    Strict,       // rejects announcements where engine report is not all_valid
}
```

### `evaluate_signature_policy`

```rust
pub fn evaluate_signature_policy(
    report: &VerificationReport,
    policy: &SignaturePolicy,
) -> Result<(), SignaturePolicyViolation>
```

| Condition | ReportOnly | Strict |
|-----------|-----------|--------|
| All resources valid | Ok | Ok |
| Empty resource list | Ok | Ok |
| No signature (Skipped) | Ok | Err |
| Invalid signature | Ok | Err |
| Unknown key ID | Ok | Err |
| Unsupported algorithm | Ok | Err |

## CLI Usage

```sh
# Offline inspection with strict policy
cargo run --bin client -- \
  --verify-announcement-signature announcement.json \
  --trusted-keys keys.toml \
  --signature-policy strict

# Live connection with strict policy
cargo run --bin client -- \
  --trusted-keys keys.toml \
  --signature-policy strict \
  --addr 127.0.0.1:7000
```

## Enforcement Behaviour

### Offline (`--verify-announcement-signature`)
- Prints plan, report, and policy verdict.
- If rejected: prints error to stderr and exits with error (via `anyhow::bail!`).
- If accepted: prints success message and exits cleanly.

### Live connection
- On `ResourceAnnouncement`: builds plan, runs engine, evaluates policy.
- If strict policy rejects: prints error to stderr, breaks out of the read loop (no availability report sent).
- No disconnect message is sent to the server — the client simply stops reading.
- If accepted: sends `ResourceAvailabilityReport` as normal.

## Test Coverage (7 new tests)

| Test | Scenario |
|------|----------|
| `policy_report_only_never_rejects` | Bad announcement under ReportOnly → Ok |
| `policy_strict_rejects_invalid` | Invalid signature under Strict → Err |
| `policy_strict_rejects_unsigned` | No signature under Strict → Err |
| `policy_strict_allows_valid` | Valid signature under Strict → Ok |
| `policy_strict_allows_empty` | Empty resource list under Strict → Ok |
| `policy_violation_message_contains_details` | Error message has expected content |
| `policy_violation_implements_debug_and_eq` | Debug/Clone/PartialEq |

## Boundaries

- **ReportOnly is the default** — no behavior change for existing workflows.
- **No silent fallback** — strict never degrades to report-only.
- **No "best effort accept"** — strict rejects if ANY resource fails.
- **No downloads, cache writes, or execution** — enforcement is purely a gate.
- **No server-side enforcement** — this milestone only adds client-side enforcement.
