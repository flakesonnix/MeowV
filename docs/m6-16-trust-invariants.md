# M6.16 Trust Invariants

## 1. State Space

The trust state machine has exactly four states:

- `Unverified`
- `Parsed`
- `PolicyChecked`
- `Trusted`

No other states may be introduced without updating this document.

## 2. Production Transition Rules

The production trust graph is a strict DAG with exactly these edges:

- `Unverified -> Parsed`
- `Parsed -> PolicyChecked`
- `PolicyChecked -> Trusted`

Each production edge maps to exactly one production function:

- `Unverified -> Parsed` via `Announcement::<Unverified>::from_raw()`
- `Parsed -> PolicyChecked` via `Announcement::<Parsed>::check_policy()`
- `PolicyChecked -> Trusted` via `Announcement::<PolicyChecked>::resolve_trust()`

`resolve_announcement_trust()` is the canonical composite production entrypoint and
must remain equivalent to:

`from_raw() -> check_policy() -> resolve_trust()`

## 3. Test-Support Edges

The `test-support` feature may extend constructor availability, but it must not
change the state graph.

Additional feature-gated edges:

- `Unverified -> Parsed` via `Announcement::<Unverified>::from_constructed()`
- `Parsed -> PolicyChecked` via `Announcement::<Parsed>::skip_policy_check()`
- `PolicyChecked -> Trusted` via `Announcement::<PolicyChecked>::trust_relaxed_for_testing()`

These edges exist only to support integration testing without external key material.
They must not be required by the default build graph.

## 4. Forbidden Transitions

The following transitions are forbidden in production and test-support builds unless
this document is updated explicitly:

- `Unverified -> Trusted`
- `Parsed -> Trusted`
- any production constructor that bypasses `check_policy()`
- any production constructor that bypasses `resolve_trust()`
- any transition from `Trusted` back to `Parsed` or `PolicyChecked`
- any `From`/`Into` conversion that mints `Announcement<Trusted>` implicitly

No alternate production semantics may exist for `PolicyChecked` or `Trusted`.

## 5. Execution Invariants

All mutation-capable repair execution entrypoints must require `Announcement<Trusted>`.

Current required boundaries:

- `plan_cache_repair()`
- `execute_cache_repair()`

Execution-layer functions must not accept `Announcement<Unverified>`,
`Announcement<Parsed>`, or `Announcement<PolicyChecked>`.

Execution-layer trust must be inherited through typed inputs, never recreated by
conversion from raw `ResourceAnnouncement` values.

## 6. Capability Boundary

The `test-support` feature is a build-time capability boundary for integration
testing only.

It must not:

- alter production policy semantics
- relax execution entrypoint requirements
- introduce new trust states
- become a required feature for default `cargo check` or `cargo test`

## 7. Canonical Entrypoints

The following functions are normative:

- `resolve_announcement_trust()` is the canonical production entrypoint
- `check_policy()` is the sole production policy gate
- `resolve_trust()` is the sole production trust escalation gate

Any new constructor or transition must be evaluated against this invariant set
before merge.
