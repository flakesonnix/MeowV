# MeowV — Agent Working Memory

## 1. Project Identity

- Clean-room GTA V-like multiplayer framework prototype
- Inspired by FiveM / alt:V / RAGE MP architecture
- Rust-first, Nix flakes, incremental Git commits
- Multi-crate Cargo workspace
- No GTA integration yet
- Branch: `milestone/0-standalone-prototype`

## 2. Hard Boundaries

- No GTA V integration
- No downloads, no file serving
- No script execution, no scripting runtime
- No Lua / JS / WASM execution
- No memory hooks, no injection
- No anti-cheat bypass
- No DRM bypass
- No Rockstar service bypass
- No proprietary / leaked / copied implementation details
- Everything: clean-room, local-only, deterministic, security-focused

## 3. Implemented Chain (in order)

1. `resource_manifest` — TOML manifest model
2. Resource pack index + SHA-256
3. Cache verification
4. Registry + dependency load order (topo sort)
5. No-exec load plan
6. No-exec runtime state machine
7. Protocol resource announcement (DTOs, server→client)
8. Client availability report + cache-verify
9. Server-side resource policy evaluation
10. Join gate dry-run decision
11. Signed resource announcement metadata stub
12. Resource compatibility rules
13. Protocol negotiation dry-run model
14. Protocol capability gate helpers
15. Server logs capability gate (report-only)
16. Client prints local capabilities
17. Server session state machine
18. `handle_client` tracks session through handshake
19. Session event log
20. Session diagnostics (`to_text`, `to_json_stub`)
21. Server config (9 sections, validation, `--config`, env overrides)
22. Structured logging config (text/JSON, log level)
23. Local admin debug commands (6 commands, stdin loop)
24. Server runtime status snapshot
25. Live session registry (BTreeMap, SessionGuard RAII)
26. Full handshake integration assertions (7 tests)
27. Admin sessions command (per-session state from registry)
28. Session enforcement dry-run planner (policy + decision types)
29. Server lifecycle config summary + smoke tests (8 tests)
30. Resource download design spec (docs-only)
31. Resource cache repair planning (report-only CLI)
32. Signature verification design spec (docs-only)
33. Signature metadata model (canonical payload, validation)
34. Signature verification dry-run planner (plan per resource)
35. Signature verification engine (real Ed25519, ed25519-dalek)
36. Wire signature verification into resource flow (CLI + live)
37. Strict signature enforcement gate (policy, evaluate)
38. Trust key UX + policy validation (key config, strict requires keys)
39. Wire session enforcement into `handle_client` (Strict disconnects)
40. Policy config sections + UX (`[signature]`, `[enforcement]`, lifecycle summary, status)

## 4. Current Active Policy

- Exact protocol version match enforced
- Protocol negotiation: dry-run only — no enforcement
- Join gate: dry-run only — no disconnect
- Signature verification: **real Ed25519 crypto**, report-only by default
- Signature enforcement: **Strict mode available**, configurable via `[signature] policy`
- Session enforcement: **Strict mode available** (disconnects on failures), configurable via `[enforcement] mode`
- Resource compatibility: report-only
- No downloads, no repair, no execution
- Client flags: `--verify-announcement-signature`, `--trusted-keys`, `--signature-policy`, `--plan-cache-repair`

## 5. Recent Milestone Status

**Milestone 4.10 — Heartbeat Admin Observability** ✅
- `SessionRegistryEntry` gains `ping_received_count` + `pong_sent_count` (cached projection of event-log-derived values)
- `SessionRegistry::update_session_heartbeat_counts()` — called after each Pong send, fed from `event_log.count_kind()`
- `SessionRegistrySnapshot::to_diagnostics_text()` — per-session line shows `ping_rx=N  pong_tx=N`
- Admin `sessions` + `diagnostics` commands show heartbeat counts without new commands
- 5 new session_registry tests + 3 new admin tests
- `docs/server-admin-debug-commands.md`, `docs/client-heartbeat.md`, `docs/roadmap.md` updated

**Invariant:** registry heartbeat counts are a cached projection of session diagnostics/event-log counts — always updated from `event_log.count_kind()`, never independently incremented.

Test state: **376 passing** (4 game_edition, 112 protocol [76 lib + 32 signature_engine + 4 e2e], 65 resource_manifest, 169 server [10 session + 8 event_log + 8 diagnostics + 30 config + 35 admin + 8 status + 25 session_registry + 21 enforcement + 15 shutdown + ~9 heartbeat], 24 integration [7 handshake_observability + 5 session_enforcement + 2 session_flow + 2 signature_verification + 8 smoke], 4 server_browser)
Working tree: clean except `AGENTS.md` (intentionally untracked)
Last commit: `3d0cdb5` — docs: document heartbeat admin observability

## 6. Important Files / Crates

| Path | Purpose |
|------|---------|
| `Cargo.toml` | workspace root, 6 member crates |
| `flake.nix` | Nix dev shell + Rust toolchain |
| `crates/protocol/src/lib.rs` | DTOs, version, policy, join gate, stub, negotiation, metadata model, dry-run planner, trust key validation — 76 tests |
| `crates/protocol/src/signature_engine.rs` | Ed25519 verification, TrustedPublicKey, VerificationReport — 32 tests |
| `crates/server/src/lib.rs` | server runtime, announcement, policy eval, join gate, mpsc writer, negotiation, session, event log, enforcement |
| `crates/server/src/session.rs` | `SessionState`, `SessionStateMachine` — forward-only transitions, 10 tests |
| `crates/server/src/event_log.rs` | `SessionEventLog`, in-memory audit trail, 8 tests |
| `crates/server/src/diagnostics.rs` | `SessionDiagnostics`, `to_text()`, `to_json_stub()`, 8 tests |
| `crates/server/src/enforcement.rs` | `SessionEnforcementPolicy`, `SessionEnforcementDecision`, `evaluate_enforcement`, 21 tests |
| `crates/server/src/config.rs` | 9-section config: server, protocol, resources, join_gate, diagnostics, logging, admin, enforcement, signature — 30 tests |
| `crates/server/src/admin.rs` | `AdminCommand`, parser, handler, status-aware handler, 31 tests |
| `crates/server/src/status.rs` | `ServerRuntimeStatus`, `from_config`, `with_session_counts`, `to_text`, 8 tests |
| `crates/server/src/session_registry.rs` | `SessionId`, `SessionRegistry`, `SessionGuard` RAII, 20 tests |
| `crates/server/src/shutdown.rs` | `ShutdownReason`, `ShutdownState`, `ShutdownSummary`, 15 tests |
| `crates/server/tests/handshake_observability.rs` | 7 integration tests for full handshake flow |
| `crates/server/tests/session_enforcement.rs` | 5 integration tests for enforcement |
| `crates/server/tests/session_flow.rs` | 2 integration tests for session flow |
| `crates/server/tests/smoke.rs` | 8 lifecycle smoke tests, no networking |
| `crates/client/src/main.rs` | CLI (--server-list, --resource-manifest, --resource-index, --verify-cache, --resource-registry, --resource-load-plan, --resource-runtime-plan, --check-resource-compatibility, --resource-cache, --protocol-negotiation, --verify-announcement-signature, --trusted-keys, --signature-policy, --plan-cache-repair) |
| `crates/resource_manifest/src/lib.rs` | manifest, pack index, cache verify, registry, load plan, runtime SM, compatibility |
| `crates/game_edition/src/lib.rs` | GameEdition, GamePlatform, detect helpers |
| `crates/server_browser/src/lib.rs` | ServerEntry, LocalJsonServerListSource, filtering |
| `examples/resources/` | chat, scoreboard, admin resource.toml fixtures |
| `examples/cache/` | chat-valid / chat-invalid cache trees |
| `docs/` | 42 docs, full reference (see `docs/roadmap.md`) |

## 7. Key Decisions

- Newline-delimited JSON protocol (reviewable, clean-room)
- SHA-256 for resource hashing
- Exact protocol version match enforced
- Resource discovery: immediate child dirs only; folder must match manifest name; no `resource.toml` → silently ignored
- Symlinks rejected in pack indexing and cache verification
- Dep resolution: Kahn/topo sort via BTreeMap/VecDeque → deterministic
- Session enforcement: Strict disconnects; ReportOnly logs only
- Signature enforcement: Strict rejects on invalid/unsigned; ReportOnly allows all
- Ed25519 via `ed25519-dalek` v2.2 for real crypto (M3.8)
- Workspace edition: 2024
- `build_availability_entries` falls back to `examples/resources/{name}` if `resource_cache` not set
- Event log: no timestamps (deterministic tests); no IP/personal data; in-memory only
- Config: relative `announcement_resource_dir` resolved from workspace root via `CARGO_MANIFEST_DIR`; absolute paths as-is
- Config validation is hard error at startup; unknown enum values rejected at TOML parse
- Strict `--signature-policy` requires `--trusted-keys <path>` or fails with clear error
- ReportOnly with no trusted keys prints informational warning
- Registry session IDs are monotonic u64 (never IP-based), deterministic in tests

## 8. Git Discipline

- Inspect `git status` before changes
- Small commits; prefer 3 per milestone:
  1. `feat:` library/model
  2. `feat:` integration/CLI
  3. `docs:`
- Lockfile: separate commit only if missed
- Never mix formatting with feature logic
- Pre-commit: `cargo fmt --all && cargo check --workspace && cargo test --workspace`
- Post-commit: `git status && git log --oneline -N`
- Do not commit `AGENTS.md` unless explicitly asked

## 9. Next Recommended Milestone

**M4.11 — Heartbeat Timeout Policy Planner**

Same pattern as signature/session enforcement: planner + decisions first, teeth later.

- `HeartbeatPolicy` enum: `ReportOnly` / `Strict`
- `HeartbeatDecision` variants: `Healthy`, `NoHeartbeatObserved`, `WouldWarnNoPongYet`, `WouldWarnTimeout`, `WouldMarkUnhealthy`, `WouldDisconnectMissedHeartbeat`
- Pure deterministic evaluator from metrics (sent_count, pong_count, timeout_or_error_count, sequences)
- `to_text()` output, unit tests, docs
- Hard boundaries: no actual disconnect, no enforcement, no protocol change
