# Resource Compatibility

## Purpose

Milestone 1.7 adds explicit report-only compatibility evaluation for resources using protocol version, edition compatibility, and platform context.

## Current Rules

- exact protocol version match required
- `any` edition matches all editions
- `legacy` matches only `legacy`
- `enhanced` matches only `enhanced`
- unknown edition context usually yields `unknown` unless resource is `any`

## Platform Compatibility

This milestone adds a small `platform_compatibility` field with `windows`, `linux`, `any`, and `unknown`. It is still a placeholder and should be treated conservatively.

## Enhanced vs Legacy Caution

Compatibility reporting does not imply GTA V runtime support. It only evaluates declared metadata against a local context.

## Unknown Behavior

Unknown context or unknown resource declarations produce `Unknown` when compatibility cannot be confirmed safely.

## Current Scope

- report only
- no join enforcement
- no downloads
- no execution
- no GTA integration

## Future Work

- richer platform metadata
- compatibility negotiation
- server-side enforcement gates
- announcement-side compatibility checks

## Clean-Room Note

Compatibility rules must remain original. Do not copy proprietary compatibility matrices, launch checks, or private platform gating logic from GTA V multiplayer ecosystems.
