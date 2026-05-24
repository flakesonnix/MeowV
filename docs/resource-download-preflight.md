# Resource Download Preflight

## Purpose

Define report-only planner for resource fetch/repair needs before any future
download milestone.

## Inputs

- `ResourceAnnouncement`
- local availability/cache result
- signature verification result + active signature policy
- resource policy evaluation, if available

## Output

Deterministic preflight plan only.

Actions:

- `AlreadyAvailable`
- `FetchMissing`
- `ReplaceInvalid`
- `BlockedBySignaturePolicy`
- `BlockedByResourcePolicy`
- `UnsupportedResource`
- `WouldVerifyAfterFetch`

## Invariants

- No network I/O
- No cache writes
- No resource execution
- No hidden mutation
- Ordering deterministic by resource, file, action
- Signature strict block wins over fetch/repair action
- Resource policy block wins over fetch/repair action
- Missing file may emit both `FetchMissing` and `WouldVerifyAfterFetch`

## Non-Goals

- No actual download protocol
- No staging directory writes
- No cache commit
- No background repair

## Follow-up Milestones

- M6.1: CLI/report command
- M6.2: source URL / fetch metadata design
- M6.3: safe fetch sandbox design
- M6.4: first explicit-flag download implementation
