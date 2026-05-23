# Signed Resource Announcements

## Purpose

Milestone 1.6 adds signature metadata and a non-enforcing signature stub for resource announcements. This prepares the protocol for future trust validation without enforcing signatures yet.

## Why Signatures Matter

Future signed announcements can help clients verify that announced resource metadata came from an expected authority and was not modified in transit.

## Current Scope

- metadata only
- no real signature verification
- no trust store
- no enforcement
- no downloads
- no execution
- no GTA integration

## What Would Be Signed Later

Future work would likely sign deterministic announcement metadata, including resource names, versions, files, sizes, hashes, and protocol-relevant announcement fields.

## Not Implemented Yet

- real cryptographic verification
- trusted key distribution
- signature enforcement
- signed download pipeline

## Trust Model Placeholder

Current model is placeholder only. `key_id` identifies an expected signer in the future, but no trust anchor or key registry exists yet.

## Key Rotation

`key_id` exists so future systems can rotate keys without redefining the whole announcement schema.

## Algorithm Agility

`algorithm` is explicit so future milestones can migrate verification algorithms without changing the protocol shape.

## Replay and Version Concerns

Future signed announcements should account for versioning, freshness, and replay resistance. This milestone does not implement those protections.

## Current Behavior

- no signature => `NotProvided`
- signature present => `NotChecked`
- flow continues unchanged

## Clean-Room Note

Signature and trust design must remain original. Do not copy proprietary trust bootstrapping, signature packaging, or launcher verification flows from GTA V multiplayer ecosystems.
