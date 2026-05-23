# Signature Verification Reporting

## Purpose

Wire the M3.7 planner and M3.8 engine into the client resource flow. Report-only: produces verification reports without changing any runtime behavior.

## CLI Flag: `--verify-announcement-signature`

Offline inspection of a `ResourceAnnouncement` JSON file:

```sh
cargo run --bin client -- --verify-announcement-signature announcement.json
cargo run --bin client -- --verify-announcement-signature announcement.json --trusted-keys keys.toml
cargo run --bin client -- --verify-announcement-signature announcement.json --trusted-keys keys.toml --reject-unsigned
```

Output format:
```
Verification Plan:
signature verification plan:
  [verify_signature] chat — algorithm 'ed25519', key 'dev-key': trusted, would verify
  total: 1 resource(s)

Verification Report:
signature verification report:
  [valid] chat
  total: 1 resource(s), all valid: true
  (report-only: no enforcement was applied)
```

## Live Connection Path

When the client connects to a server and receives a `ResourceAnnouncement`:

1. The stub report (`check_announcement_signature_stub`) is always printed.
2. If `trusted_keys_file` is configured (via TOML config, `--trusted-keys`, or `MEOWV_TRUSTED_KEYS` env var), the engine also runs and its report is printed.

```sh
cargo run --bin client -- --trusted-keys keys.toml --addr 127.0.0.1:7000
```

Live output:
```
Announcement Signature Status (stub): not_checked
Stub Reason: signature metadata is valid: algorithm 'ed25519', key 'dev-key'; ...
Signature Verification Report (engine):
signature verification report:
  [valid] chat
  total: 1 resource(s), all valid: true
  (report-only: no enforcement was applied)
```

## Trusted Keys Config Format

```toml
[[trusted_key]]
key_id = "dev-key"
algorithm = "ed25519"
public_key_b64 = "GU9rI+FshTLGq8g4+s1ep4m+DHaykgQzA5v6iz02jWE="

[[trusted_key]]
key_id = "prod-key"
algorithm = "ed25519"
public_key_b64 = "7s8H3lWqZ9xAbCdEfGhIjKlMnOpQrStUvWxYz12345="
```

Each entry has:
- `key_id` — matches `key_id` in `ResourceAnnouncementSignature`
- `algorithm` — signature algorithm (currently only `"ed25519"`)
- `public_key_b64` — base64-encoded raw 32-byte Ed25519 public key

## Client Config Integration

```toml
# client.toml
addr = "127.0.0.1:7000"
name = "dummy-client"
trusted_keys_file = "keys.toml"
```

Or via env var:
```sh
MEOWV_TRUSTED_KEYS=keys.toml cargo run --bin client
```

## Test Coverage (5 new end-to-end tests)

| Test | Scenario |
|------|----------|
| `full_flow_plan_and_engine_valid` | Plan says VerifySignature, engine says Valid |
| `full_flow_reject_unsigned` | Plan says WouldRejectUnsigned, engine says Skipped |
| `full_flow_no_trusted_keys_skipped` | Orphan key → UnknownKeyId plan → Invalid outcome |
| `report_to_text_contains_all_fields` | All expected markers in to_text() output |
| `report_to_text_empty` | Empty resource list → vacuous all_valid |

## Boundaries

- No enforcement — all output is report-only
- No disconnects, downloads, cache writes, or execution
- Engine runs only when trusted keys are provided; stub always runs
- `reject_unsigned` is a CLI/config input that affects the plan but does not change live behavior
