# Server Resource Policy

## Purpose

Milestone 1.4 adds server-side evaluation of `ResourceAvailabilityReport`. The server computes whether a client should be considered allowed, warning-only, or blocked based on announced resource requirements.

## Decisions

- `Allowed`: all announced files available
- `WarningOnly`: required files available, but optional or recommended files missing or invalid
- `Blocked`: one or more required files missing or invalid

## Current Scope

- local only
- no downloads
- no repair
- no execution
- no GTA integration
- no disconnect enforcement yet

## Missing Report Entries

If the server announced a file and the client omits it from the report, that file is treated as `Missing`.

## Extra Report Entries

Extra client report entries not present in the announcement are ignored in this milestone.

## Requirement Levels

- `Required`: missing or invalid => `Blocked`
- `Optional`: missing or invalid => `WarningOnly`
- `Recommended`: missing or invalid => `WarningOnly`

## Why Blocked Is Not Enforced Yet

This milestone only computes and logs the policy decision. Enforcement remains a later control point so policy logic can be validated first without affecting connection flow.

## Future Enforcement Point

Later work may use this evaluation to gate session readiness, delay runtime startup, or disconnect clients once the project is ready for explicit enforcement behavior.

## Clean-Room Note

Server-side resource policy must remain original. Do not copy proprietary enforcement logic, patch gating behavior, or launcher policy flows from GTA V multiplayer ecosystems.
