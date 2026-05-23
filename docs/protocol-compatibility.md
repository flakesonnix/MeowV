# Protocol Compatibility

## Current Policy

- Current standalone prototype protocol version: `1`
- Server and client must match exactly in Milestone 0
- Mismatch returns clean `disconnect` packet with `protocol_mismatch`

## Why Strict Matching Now

Milestone 0 optimizes for clarity and reviewability, not long-lived compatibility. Exact matching avoids hidden fallback logic while protocol shape is still unstable.

## Planned Evolution

Future versions should move to explicit negotiation instead of strict equality.

Expected steps:

1. client sends supported version range or capability list
2. server selects compatible version
3. server disconnects only when no safe overlap exists

## Change Rules

- bump protocol version for breaking wire-format changes
- document all packet changes in release notes
- add tests for both success and rejection paths
- avoid silent field repurposing

## Clean-Room Note

Protocol evolution must remain original. Do not copy negotiation behavior or wire schemas from proprietary or third-party GTA V multiplayer systems.
