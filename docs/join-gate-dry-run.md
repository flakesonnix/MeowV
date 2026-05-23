# Join Gate Dry-Run

## Purpose

Milestone 1.5 adds a dry-run join gate decision layer. After resource policy evaluation, the server computes what would happen if enforcement were enabled.

## Dry-Run vs Enforced

- `DryRun`: compute and report only
- `Enforced`: reserved for future behavior

This milestone always uses `DryRun`.

## Outcome Mapping

- `Allowed` -> `WouldAllow`
- `WarningOnly` -> `WouldWarn`
- `Blocked` -> `WouldBlock`

## Current Behavior

- server evaluates resource report
- server builds `JoinGateDecision`
- server logs the decision
- server may send decision to client
- server does not disconnect
- server does not block networking

## Why No Disconnection Yet

Dry-run mode lets policy and UX be validated before any enforcement path changes connection behavior.

## Current Scope

- local only
- no downloads
- no repair
- no execution
- no GTA integration

## Future Enforcement Point

Later work may use `JoinGateMode::Enforced` to block session readiness or disconnect clients once explicit enforcement behavior is reviewed and tested.

## Clean-Room Note

Join gate behavior must remain original. Do not copy proprietary session gating, patch enforcement, or launcher block/warn flows from GTA V multiplayer ecosystems.
