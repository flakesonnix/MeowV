# Legal Boundaries

## Allowed Scope

- Clean-room networking architecture
- Original protocol design
- Standalone server and dummy client
- Open-source tooling, docs, and test harnesses
- Future launcher/resource systems that do not bypass platform protections

## Disallowed Scope

- Anti-cheat bypasses
- DRM bypasses
- Rockstar Online service interference
- Proprietary or leaked code
- Copied packet formats from FiveM, alt:V, RAGE Multiplayer, GTA V, or Rockstar services
- Injection, hooking, or memory patching in this milestone
- Shipping copyrighted assets or data extracted from game files

## Risk Notes

High-risk areas for future review:

- any bridge into GTA V process space
- loading external code into game runtime
- replication formats derived from observed proprietary traffic
- launcher behavior that might alter platform security assumptions

## Safer Alternatives

- Prototype protocols against dummy clients first
- Keep transport and replication logic engine-agnostic
- Document original packet schemas and rationale
- Add explicit review checkpoints before any native integration work
