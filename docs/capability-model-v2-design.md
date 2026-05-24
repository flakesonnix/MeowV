# Capability Model v2 Design (M5.0)

## Purpose

Define next protocol capability negotiation model before any wire-format change.
This is design only. M5.0 changes no DTOs, runtime behavior, or enforcement.

## Current State

- `Login` carries only `name` and exact `protocol_version`
- Exact protocol version match is still hard-enforced
- Protocol negotiation exists as dry-run model only
- Capability gate helpers exist, but client does not declare capabilities
- Shared capability set is therefore effectively empty in live handshake flow

Current gap: capability concepts exist, but handshake cannot express what client
actually supports.

## Design Goals

- Make client feature support explicit in handshake
- Separate wire compatibility from feature support
- Keep required vs optional capabilities distinct
- Preserve deterministic, observable negotiation results
- Keep legacy-client handling explicit, not implicit

## Proposed Login Payload v2

Proposed future handshake shape:

```rust
Login {
    name: String,
    protocol_version: u32,
    capabilities: Vec<ProtocolCapability>,
    feature_flags: Option<Vec<String>>,
}
```

Design notes:

- `protocol_version` remains wire-compatibility gate
- `capabilities` carries typed protocol features already modeled in `protocol`
- `feature_flags` is optional, string-based, reserved for non-core experimental or
  implementation-level toggles that should not become first-class protocol
  capabilities immediately

Rules:

- `capabilities` should be sorted + deduplicated by receiver during validation
- Unknown `feature_flags` are tolerated unless explicitly required by policy
- Unknown typed capabilities should not be representable once DTO enum is bumped;
  old peers hit version mismatch before decode ambiguity matters

## Capability Categories

Two server policy classes:

### Required capabilities

Needed for session to proceed with selected feature flow.

Examples:

- `ResourceAnnouncement`
- `ResourceAvailabilityReport`
- future authoritative handshake features

If required capability is absent:

- negotiation result = rejected
- reason identifies missing capability deterministically

### Optional capabilities

Improve session fidelity or enable extra observability, but absence does not
block session.

Examples:

- `JoinGateDryRun`
- `ResourceCompatibilityReport`
- future advisory-only reports

If optional capability is absent:

- negotiation result = accepted with warnings
- feature flow may be skipped or downgraded explicitly

## Proposed Server Policy Model

Future server config/policy shape:

```toml
[capabilities]
required = ["resource_announcement", "resource_availability_report"]
optional = ["join_gate_dry_run", "resource_compatibility_report"]
unknown_client_capabilities = "warn"
legacy_client_without_capabilities = "reject"
```

Design intent:

- `required`: hard gate list
- `optional`: warning / degraded-session list
- `unknown_client_capabilities`: how server reacts when newer client advertises
  capability unknown to this server generation
- `legacy_client_without_capabilities`: explicit compatibility mode for pre-v2
  clients

Recommended semantics:

- unknown advertised capability: warn, ignore for enforcement, record in diagnostics
- missing required capability: reject
- missing optional capability: accept with warning

## Unknown / Unsupported Capability Behavior

Three cases matter:

1. Client advertises capability server does not understand
- recommended result: accepted with warning
- server ignores capability for gating
- diagnostic/log line records unknown capability strings

2. Server requires capability client does not advertise
- result: rejected
- structured reason: `missing_required_capability:<name>`

3. Client sends no capability payload at all
- treated as legacy client, not as empty capability list
- handling depends on protocol version / legacy policy

Reason for separating case 3:
- missing field means old handshake schema
- empty list means new schema explicitly declares no capabilities

## Compatibility / Version Strategy

This design should use a protocol version bump.

Reason:

- current `Login` DTO has no capability field
- adding required structured handshake data is wire-format change
- exact version match is currently hard-enforced anyway

Recommended path:

1. Bump `PROTOCOL_VERSION`
2. Add `Login` capability payload in new version only
3. Treat older clients as legacy protocol peers rejected by exact version check
4. Optionally keep server-side doc-only policy for future mixed-version support,
   but do not pretend current stack supports cross-version negotiation

Short version: capability model v2 should be introduced alongside protocol v2,
not as hidden extension of protocol v1.

## Negotiation Result Model

Future explicit result shape:

```rust
enum CapabilityNegotiationResult {
    Accepted,
    AcceptedWithWarnings { warnings: Vec<String> },
    Rejected { reason: String },
}
```

Behavior:

- `Accepted`: all required capabilities present, no noteworthy warnings
- `AcceptedWithWarnings`: session proceeds but missing optional or unknown client
  capabilities are recorded
- `Rejected`: required capability missing, malformed declaration, or explicit
  legacy-client policy says reject

Result should be deterministic:

- warnings sorted
- capability names normalized to stable string form
- rejection reason stable for tests/admin output

## Observability Requirements

When implemented later, expose negotiation through existing surfaces:

- session diagnostics
  - advertised client capabilities
  - required / optional server capability sets
  - negotiation result
  - warning list / rejection reason
- admin `sessions`
  - short capability summary or result label
- admin `status`
  - active capability policy mode / required capability count
- structured logs
  - client capability count
  - shared capability count
  - negotiation result
  - rejection reason if any

No personal data needed. Keep output deterministic.

## Legacy Client Policy

Recommended default for first live wire-change milestone:

- missing capability payload on new protocol version: reject as malformed login
- old protocol version: rejected by exact version mismatch before capability logic

Future relaxed mixed-version support can be designed later, but should not be
claimed in first v2 rollout.

## Recommended Milestone Split

### M5.0
- design doc only

### M5.1
- add `Login` v2 DTO fields
- bump protocol version
- add decode/encode tests
- bump example resource manifests to protocol v2 so exact-version handshake fixtures still announce resources

### M5.2
- add pure capability negotiation result evaluator
- no runtime enforcement yet

### M5.3
- wire capability result into server handshake diagnostics / logs

### M5.4
- optional policy enforcement for required capabilities

## Non-Goals

- no runtime negotiation in M5.0
- no protocol wire change in M5.0
- no heartbeat/session/signature behavior changes
- no mixed-version support promise yet
- no new dependencies
