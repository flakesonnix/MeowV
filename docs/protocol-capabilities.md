# Protocol Capabilities

## Purpose

Protocol capabilities describe discrete features that a peer supports beyond the
base protocol version. They are separate from the protocol version number:

- **Protocol version** — determines wire format compatibility and message set.
- **Protocol capabilities** — declare which optional feature flows a peer can
  participate in.

A peer may share the same protocol version as another yet still lack support for
a specific capability (e.g., a minimal client that does not implement resource
announcement handling). Capabilities allow this distinction without bumping the
version.

## Defined Capabilities

| Capability | Description |
|---|---|
| `ResourceAnnouncement` | Peer can send or receive resource announcement messages. |
| `ResourceAvailabilityReport` | Peer can produce or consume resource availability reports. |
| `JoinGateDryRun` | Peer supports the join gate dry-run decision flow. |
| `ResourceCompatibilityReport` | Peer can evaluate and report resource compatibility. |
| `SignatureMetadata` | Peer can carry and inspect resource announcement signature metadata. |

## Current Behaviour: Report-Only Capability Gates

Capability gating is **dry-run and report-only** in this milestone. No
enforcement occurs:

- The server checks its own capability profile before sending a
  `ResourceAnnouncement` and before sending a `JoinGateDecision`.
- The result is logged (`supported`, `reason`) at `info` level.
- The message is sent regardless of the gate result.
- The client is never disconnected or blocked based on capability checks.

The `shared_capabilities` helper computes the intersection of two peer profiles.
Because the client does not advertise capabilities in the `Login` message, the
server's shared set is currently empty. The gate reports therefore log
`supported = false` with a reason string that makes the report-only status
explicit.

This is intentional and accurate: the mechanism is wired up and observable in
logs, but has no effect on session behaviour.

## Helper API (`crates/protocol`)

```rust
// Check whether a single profile declares a capability.
profile_supports_capability(profile, capability) -> bool

// Compute the sorted, deduplicated intersection of two profiles.
shared_capabilities(client, server) -> Vec<ProtocolCapability>

// Return Ok if capability is present in the shared set, Err otherwise.
requires_capability(capability, shared) -> Result<(), ProtocolCapabilityError>

// Build a CapabilityGateReport (supported flag + reason string).
capability_gate_report(capability, shared) -> CapabilityGateReport
```

## Future Behaviour

When real protocol negotiation is activated (a future milestone):

- The client will advertise its capabilities in the handshake.
- `shared_capabilities` will return a non-empty intersection for matching peers.
- `requires_capability` will be used to gate feature flows conditionally.
- Peers missing a required capability may be offered a degraded session or
  rejected, depending on the feature's requirement level.
- This change will be gated behind an explicit policy flag; the current
  exact-version-match policy will remain default until then.

## Why Exact Version Match Remains Active

Capability profiles extend the negotiation model but do not replace the current
exact-match enforcement. The server still disconnects clients that present a
different `protocol_version` in the `Login` message. Capability negotiation is
designed for a later phase when the protocol is stable enough to support
multiple active versions.

## Hard Boundaries

This feature does not and will not:

- Enable real protocol version negotiation (still dry-run).
- Disconnect or degrade clients based on capability checks (still report-only).
- Add downloads, file serving, or resource repair.
- Add script execution or any scripting runtime.
- Integrate with GTA V or any proprietary system.
- Use leaked, proprietary, or copied implementation details.

All capability logic is clean-room, local-only, and produces no side-effects
beyond structured log output.
