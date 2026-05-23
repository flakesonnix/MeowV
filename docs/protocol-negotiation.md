# Protocol Compatibility Negotiation (Dry-Run)

## Current Active Policy: Exact Version Match

As of Milestone 0 / 1.8, the server and client enforce a strict exact
protocol version match. If `client.protocol_version != PROTOCOL_VERSION`,
the server sends a `Disconnect` with reason `ProtocolMismatch` and
closes the connection. This policy has not changed.

## Dry-Run Negotiation Model

This milestone adds a **dry-run** negotiation layer that computes what
a future compatibility negotiation *would* decide, without changing the
active enforcement policy.

### Data Structures

- **`ProtocolVersionRange`**: `{ min: u32, max: u32 }` — the set of
  wire-format versions a peer claims to support.

- **`ProtocolCapability`**: Enumeration of known protocol feature flags:
  - `ResourceAnnouncement`
  - `ResourceAvailabilityReport`
  - `JoinGateDryRun`
  - `ResourceCompatibilityReport`
  - `SignatureMetadata`

- **`ProtocolCompatibilityProfile`**: A peer's advertised profile
  containing a `version_range` and a `capabilities` list.

- **`ProtocolNegotiationStatus`**: One of:
  - `ExactMatch` — both ranges contain `PROTOCOL_VERSION`
  - `CompatibleDryRun` — ranges overlap but neither contains the
    current exact version
  - `Incompatible` — no overlap

- **`ProtocolNegotiationResult`**: The computed result with status,
  selected version, shared capabilities (intersection, sorted),
  and a human-readable reason.

### Key Functions

- `current_protocol_profile()` — returns the profile for the current
  build (single-version range `[PROTOCOL_VERSION, PROTOCOL_VERSION]`,
  all known capabilities).

- `protocol_ranges_overlap(a, b)` — true if the two ranges intersect.

- `negotiate_protocol_dry_run(client, server)` — computes the dry-run
  result:
  1. If both ranges contain `PROTOCOL_VERSION` → `ExactMatch` with
     `selected_version = PROTOCOL_VERSION`.
  2. If ranges overlap but neither contains `PROTOCOL_VERSION` →
     `CompatibleDryRun` with `selected_version = highest shared`.
  3. If no overlap → `Incompatible` with `selected_version = None`.
  4. `shared_capabilities` is the sorted, deduplicated intersection.

### Why Negotiation Is Not Enabled

1. **Protocol shape is unstable**. While the wire format is still
   changing, negotiation logic would add hidden fallback paths that
   make review harder.

2. **No multi-version server yet**. The server only speaks one version.
   Negotiation makes sense when a server must simultaneously support
   multiple client versions.

3. **Security-first principle**. Exact match is the easiest policy to
   audit. A future negotiation activation must be reviewed separately.

### Future Activation Plan

1. Remove or relax the hard protocol mismatch disconnect.
2. On login, call `negotiate_protocol` (not dry-run) and pass the
   selected version to the session.
3. Route message encoding/decoding through version-specific dispatch.
4. Test all version combinations.
5. Document wire-format changes in release notes.

### Compatibility Risks

- Forward compatibility is harder than backward. A client newer than the
  server cannot safely assume the server knows about new message types.
- Capability intersection is purely advisory. A capability's absence
  does not guarantee the peer cannot handle it; the list must be
  kept in sync with the actual implementation.
- Protocol negotiation adds state to the handshake. Every new version
  range increases the test matrix.

### Clean-Room Warning

All negotiation types and algorithms are original designs produced
without reference to any proprietary or third-party GTA V multiplayer
system (including but not limited to FiveM, cfx.re, alt:V, RageMP, or
Rockstar's own matchmaking). Do not copy negotiation behavior or wire
schemas from any of those systems.

### No GTA V Integration

This module operates entirely within the standalone protocol layer.
It does not:
- Read, write, or process GTA V game data
- Detect, launch, or interact with GTA V processes
- Load or execute any game scripts
- Bypass any anti-cheat, DRM, or Rockstar service
