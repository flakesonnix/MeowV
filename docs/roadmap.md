# Roadmap

## Milestone Summary

### 0.x — Foundations

| MS | Title |
|---|---|
| 0 | Standalone prototype — Rust workspace, Nix, protocol, server, dummy client, login/chat/entity sync |
| 0.5 | Game edition layer — edition-aware types, platform detection, clean-room docs |

### 1.x — Resource / Protocol Dry-Run Pipeline

| MS | Title |
|---|---|
| 1.0 | Resource registry — discovery, dep resolution, cycle detection, load order |
| 1.1 | Runtime boundary — no-exec planning, deterministic resource ordering |
| 1.2 | Runtime state machine — no-exec lifecycle, dependency readiness |
| 1.8 | Protocol negotiation dry-run — version ranges, capabilities, intersection logic |
| 1.9 | Capability-gated resource flow — gate helpers, server/client dry-run reporting |

### 2.x — Server Runtime / Admin / Debug Infrastructure

| MS | Title |
|---|---|
| 2.0 | Session state machine — Connected→ReadyDryRun/Failed, forward-only |
| 2.1 | Session event log — in-memory per-session audit trail |
| 2.2 | Session diagnostics — read-only snapshot from SM + event log |
| 2.3 | Server config — 5-section TOML config, validation, dry-run policies |
| 2.4 | Structured logging — LogLevel/LogFormat, text/JSON, config-driven |
| 2.5 | Local admin commands — stdin parser, 6 commands, oneshot quit |
| 2.6 | Runtime status snapshot — ServerRuntimeStatus, admin status/sessions live |
| 2.7 | Live session registry — BTreeMap-backed, SessionGuard RAII |
| 2.8 | Full handshake integration assertions — session, event log, registry, diagnostics consistent |
| 2.9 | Admin sessions command — registry-backed per-session state listing |

### 3.x — Next Phase Candidates

| MS | Title |
|---|---|
| 3.0 | Session enforcement dry-run — policy model, decision types, report-only planner |
| 3.1 | Server lifecycle config — summary text, startup logging, coverage |
| 3.2 | Server lifecycle smoke test — config/runtime init, example config, no flaky networking |
| 3.3 | Resource download design spec — threat model, protocol, staging, signatures |
| 3.4 | Resource cache repair planning — report-only, no downloads, no mutation |
| 3.5 | Real signature verification design — design spec for announcement/resource index signature verification |
| 3.6 | Signature metadata model — typed algorithm enum, structural validation, canonical payload |
| 3.7 | Signature verification dry-run planner — deterministic plan/report from metadata + trust policy |
| 3.8 | Signature verification engine — real Ed25519 crypto, consumes M3.7 plan, report-only |
| 3.9 | Wire signature verification into resource flow — client CLI + live path, report-only |
| 4.0 | Strict signature enforcement gate — `SignaturePolicy`, evaluate + reject under strict, report-only by default |
| 4.1 | Trust key UX + policy validation — `KeyConfigError`, validate config, strict requires keys, key summary |
| 4.2 | Wire session enforcement into `handle_client` — Strict policy disconnects, ReportOnly preserved, integration tests |
| 4.3 | Policy config sections + UX — `[signature]` section, `[enforcement]` validated, status/admin output, lifecycle summary, example config, docs |
| 4.4 | Heartbeat / Ping-Pong Protocol — add Ping/Pong DTOs; server echoes Pong(sequence); tests |
| 4.7 | Configurable periodic heartbeat loop — client-side periodic Ping; report-only, no enforcement |
| 4.9 | Heartbeat status / metrics — `HeartbeatMetrics` return from client loop; client prints summary on shutdown; server tracks `ping_received_count`/`pong_sent_count` via `SessionDiagnostics` |
| 4.10 | Heartbeat admin observability — surface heartbeat counts in `sessions`/`diagnostics` admin output via registry snapshot; tests; docs |
| 4.11 | Heartbeat timeout policy planner — `HeartbeatPolicy`, `HeartbeatDecision`, `HeartbeatPlannerInput`; deterministic evaluator; surfaced in `SessionDiagnostics`; report-only by default |
| 4.12 | Heartbeat policy report in admin/status — `heartbeat=<label>` per-session in `sessions`/`diagnostics` output; `to_short_label()` on `HeartbeatDecision`; tests; docs |
| 4.13 | Heartbeat policy config plumbing — `[heartbeat]` TOML section; `HeartbeatSection` with `Default → ReportOnly`; policy threaded through registry snapshot, diagnostics, admin output, and `ServerRuntimeStatus`; no enforcement |
| 4.14 | Strict heartbeat enforcement wiring — `ClientHeartbeatPolicy` enum; client-side `heartbeat_loop` enforces disconnect at threshold under `Strict`; `--heartbeat-policy` CLI flag; `enforcement_disconnect` field on `HeartbeatMetrics`; 7 new tests |
| 4.15 | Heartbeat authority design — document why server-side enforcement is unreachable today; compare client-reported health vs server-initiated Ping; recommend server-initiated path for future enforcement; design doc only, no live changes |
| 4.16 | Server-initiated heartbeat protocol stub — add `ServerMessage::ServerPing` and `ClientMessage::ServerPong` DTOs; server handler ignores `ServerPong` (inert); 8 round-trip tests; no timer, no enforcement |
| 4.17 | Client responds to ServerPing — client receive loop and heartbeat path reply `ServerPong(sequence)` to `ServerPing(sequence)`; `handle_server_ping` public helper; 5 integration tests |
| 4.18 | Server-side ServerPing scheduler, report-only — per-session `interval_at` timer sends `ServerPing` after handshake; `ServerPong` replies recorded; `srv_ping_tx` / `srv_pong_rx` in registry + diagnostics; `server_ping_interval_ms` config; 7 integration tests; no enforcement |
| 4.19 | Server-side heartbeat timeout status / planner — `ServerHeartbeatPlannerInput`, `ServerHeartbeatDecision` (NoActivity/Healthy/AwaitingPong/MissedPong/WouldDisconnect), `evaluate_server_heartbeat`; `srv_heartbeat=<label>` in registry diagnostics and admin sessions; `server_heartbeat_decision` in `SessionDiagnostics`; 15 planner unit tests + 7 registry/admin unit tests + 6 integration tests; no enforcement |
| 4.20 | Strict server-side heartbeat enforcement — `Strict` policy + `WouldDisconnect` decision → clean disconnect in scheduler tick arm; `SessionEventKind::Failed` recorded with structured reason; registry updated to `Failed` before removal; diagnostics emitted on enforcement; `ReportOnly` unchanged; 5 integration tests |
| 4.21 | Heartbeat enforcement polish / invariants — code comments documenting best-effort Disconnect vs guaranteed EOF (writer_half drop); `handle_enforcement` doc note for direct-write path; `heartbeat-authority-design.md` implementation status; 1 deterministic integration test (`strict_enforcement_independent_of_client_ping_activity`) |
| 4.22 | Milestone stack audit / release notes — concise M4.0-M4.21 summary doc covering enforcement, signature, client heartbeat, server-authoritative heartbeat, admin visibility, ReportOnly vs Strict behavior, and Disconnect-vs-EOF guarantees; roadmap updated; no runtime changes |
| 5.0 | Capability model v2 design — define Login capability payload, required vs optional capability policy, unknown capability behavior, protocol version bump strategy, explicit negotiation result, and observability plan; design doc only, no wire changes |
| 5.1 | Login capability payload + protocol version bump — `Login` carries required/optional capabilities and optional feature flags; protocol bumped to v2; client sends payload; server reads/stores payload; legacy missing-payload login rejected |
| 5.2 | Capability negotiation report / dry-run gate — deterministic accepted / accepted_with_warnings / would_reject report from `LoginCapabilities` and server policy; surfaced in diagnostics/admin/status; no disconnect enforcement |

---

## Milestone 4.22

Milestone stack audit / release notes:

- New summary doc: `docs/m4-enforcement-heartbeat-summary.md`
- Captures current M4.0-M4.21 guarantees for:
  - session enforcement
  - signature policy
  - client-initiated heartbeat
  - server-authoritative heartbeat
  - admin / diagnostics visibility
  - ReportOnly vs Strict behavior
  - best-effort `Disconnect` frame vs guaranteed TCP close / EOF
- Cross-links existing detailed docs without changing runtime or protocol behavior
- No code changes, no test changes, no enforcement changes

---

## Milestone 5.0

Capability model v2 design:

- New design doc: `docs/capability-model-v2-design.md`
- Defines future `Login` capability payload and optional feature-flag extension point
- Defines server policy shape for required vs optional capabilities
- Defines unknown capability and legacy-client behavior
- Recommends protocol version bump for first live rollout of capability payload
- Defines explicit negotiation result model: accepted / accepted_with_warnings / rejected
- Defines diagnostics, admin, and structured-log observability targets
- No protocol wire change, no runtime behavior change, no enforcement change

---

## Milestone 5.1

Login capability payload + protocol version bump:

- `PROTOCOL_VERSION` bumped from `1` to `2`
- `ClientMessage::Login` now carries `capabilities: LoginCapabilities`
- `LoginCapabilities` separates `required`, `optional`, and optional string `feature_flags`
- Login decode normalizes capability lists and feature flags via sort + dedup for deterministic behavior
- Unknown typed capability strings are rejected at decode; unknown feature flags are tolerated
- Client login creation now sends current required/optional capability sets on live connect and `--ping-once`
- Server login handling reads/stores advertised login capabilities and includes counts in observability
- Missing capability payload on protocol v2 login is rejected as `InvalidHandshake`
- Example resource manifests bumped to protocol v2 so handshake announcement fixtures still build under exact-version tests
- No required-capability enforcement yet; exact protocol match remains active gate

---

## Milestone 5.2

Capability negotiation report / dry-run gate:

- New deterministic protocol-layer negotiation report from advertised `LoginCapabilities` and server capability policy
- Result labels: `accepted`, `accepted_with_warnings`, `would_reject`
- Reports required-supported, required-missing, optional-supported, optional-missing, unsupported client optional capabilities, and feature flags
- Missing required capabilities are recorded as would-reject violations only; live handshake still proceeds in this milestone
- Report is surfaced read-only through session diagnostics, live session registry/admin `sessions`, and server `status`
- No protocol wire changes, no disconnects, no strict capability enforcement yet

---

## Milestone 4.21

Heartbeat enforcement polish / invariants:

- Code comments in `lib.rs` around the scheduler tick enforcement path:
  - Before `client_tx.send(Disconnect)`: documents best-effort nature — queued message may not flush before `writer_task.abort()` preempts the writer
  - Before `writer_task.abort()`: documents authoritative TCP close — abort drops `writer_half`, delivering EOF on all loop exit paths
  - `handle_enforcement` doc extended with note that it is called pre-spawn with direct write access (guaranteed delivery)
- `docs/heartbeat-authority-design.md` updated with implementation status section covering M4.16–M4.20 outcomes and confirming Option B (server-initiated Ping/Pong) was implemented as designed
- One new deterministic integration test: `strict_enforcement_independent_of_client_ping_activity` — client sends client-initiated Pings throughout but never replies to `ServerPong`; verifies Strict enforcement fires on the server-initiated direction independently
- No protocol changes, no flaky tests on Disconnect frame delivery, no enforcement behavior changes

---

## Milestone 4.20

Strict server-side heartbeat enforcement:

- Enforcement wired into the `srv_ping_interval.tick()` arm of the post-handshake `select!` loop
- After each `ServerPing` is sent and counts updated, evaluates `evaluate_server_heartbeat(&input, &HeartbeatPolicy::Strict)` when `HeartbeatPolicy::Strict` is active
- If `ServerHeartbeatDecision::WouldDisconnect` (missed ≥ `MISSED_SERVER_PONG_DISCONNECT_THRESHOLD`):
  1. `session.fail(reason)` — state machine transitions to `Failed`
  2. `warn!` with `pings_sent`, `pongs_received`, `missed` fields
  3. `event_log.record(ServerEventKind::Failed, ...)` with structured reason
  4. `registry.update_session(Failed, event_count)` — registry reflects failure before removal
  5. Optional diagnostics emit (if `print_session_diagnostics` enabled)
  6. `client_tx.send(Disconnect { reason: InvalidHandshake, message })` — best-effort; writer task may or may not deliver before abort
  7. `break` — exits the main loop; cleanup + `SessionGuard` drop removes session from registry
- `ReportOnly` policy: enforcement block skipped entirely; behavior identical to M4.19
- `srv_heartbeat=would_disconnect` label is now a transient state under Strict (session is removed before it can be observed in a snapshot)

---

## Milestone 4.19

Server-side heartbeat timeout status / planner:

- `ServerHeartbeatPlannerInput { pings_sent: u64, pongs_received: u64 }` — pure input struct for server-initiated direction
- `MISSED_SERVER_PONG_DISCONNECT_THRESHOLD = 3` — mirrors client-side threshold
- `ServerHeartbeatDecision` variants: `NoActivity`, `Healthy`, `AwaitingPong`, `MissedPong`, `WouldDisconnect`
- `evaluate_server_heartbeat(input, policy)` — pure deterministic planner; `Strict` escalates to `WouldDisconnect` when `missed >= threshold`; `ReportOnly` never escalates beyond `AwaitingPong`/`MissedPong`
- `ServerHeartbeatDecision::to_short_label()` → `"no_activity"` / `"healthy"` / `"awaiting_pong"` / `"missed_pong"` / `"would_disconnect"`
- `SessionRegistrySnapshot::to_diagnostics_text()` extended with `srv_heartbeat=<label>` per session (computed from `server_ping_sent_count` / `server_pong_received_count`)
- `SessionDiagnostics` gains `server_heartbeat_decision: Option<String>`; `with_heartbeat_policy()` now evaluates both client-initiated and server-initiated decisions in one call; `to_text()` emits `server_heartbeat_decision: <label>`; `to_json_stub()` includes field
- Admin `sessions` output automatically includes `srv_heartbeat=<label>` via `to_diagnostics_text()`
- All new items pub-exported from `server` crate
- No actual disconnect, no enforcement; strict-policy WouldDisconnect is a planning label only

---

## Milestone 4.18

Server-side ServerPing scheduler, report-only:

- `server_ping_interval_ms: u64` added to `HeartbeatSection` in config (default: 5000; 0 = disabled)
- `SessionEventKind::ServerPingSent` and `ServerPongReceived` added to event log
- `SessionRegistryEntry` gains `server_ping_sent_count` and `server_pong_received_count`
- `SessionRegistry::update_server_heartbeat_counts()` — call after each ServerPong received
- `SessionRegistrySnapshot::to_diagnostics_text()` includes `srv_ping_tx=N  srv_pong_rx=N` per session
- `SessionDiagnostics` gains `server_ping_sent_count` and `server_pong_received_count`; surfaced in `to_text()` and `to_json_stub()`
- Post-handshake loop in `handle_client` converted to `loop { tokio::select! { ... } }` with `interval_at(now+dur, dur)` timer branch guarded by `if srv_ping_enabled`
- `ClientMessage::ServerPong` arm now active: records `ServerPongReceived` event, updates registry
- `example.server.toml` updated with `server_ping_interval_ms = 5000`
- 7 integration tests: server sends ping after handshake, sequences increment, pong reply updates counts, mismatched pong not fatal, report-only never disconnects, cleanup stops scheduler, diagnostics shows counts
- No strict enforcement, no disconnect on missed pong

---

## Milestone 4.17

Client responds to ServerPing:

- `client::handle_server_ping(writer, sequence)` — public async helper in `lib.rs`; sends `ClientMessage::ServerPong { sequence }`
- Main receive loop in `main.rs` handles `ServerMessage::ServerPing` arm; replies and logs at info level
- `heartbeat::send_ping_and_wait_with_timeout` intercepts interleaved `ServerPing` while waiting for a `Pong`; replies inline and continues waiting
- 5 integration tests (`tests/server_heartbeat.rs`): single ping elicits pong, multiple pings get matching pongs in order, sequence 0 round-trips, ServerPing interleaved during heartbeat wait is handled transparently, unrelated server messages not disrupted
- No server-side timer, no timeout tracking, no disconnect, no enforcement
- Docs: `client-heartbeat.md` updated; `roadmap.md` updated

---

## Milestone 4.16

Server-initiated heartbeat protocol stub:

- `ServerMessage::ServerPing { sequence: u64 }` — server-to-client authoritative liveness ping (inert)
- `ClientMessage::ServerPong { sequence: u64 }` — client echo of `ServerPing` (inert)
- Server `handle_client` logs received `ServerPong` at info level; no enforcement action
- 8 protocol round-trip tests: `ServerPing` and `ServerPong` serialize/deserialize, sequence edge cases (0, u64::MAX), existing `Ping`/`Pong` unaffected, distinct serde `type` fields
- No live server timer, no timeout tracking, no enforcement, no disconnect
- Docs: `heartbeat-authority-design.md` updated; `client-heartbeat.md` updated with two-direction model

---

## Milestone 4.15

Heartbeat authority design (docs only):

- Documents why `WouldDisconnectMissedHeartbeat` is unreachable server-side today (server always has `timeout_or_error=0`)
- Compares Option A (client-reported `HeartbeatHealthReport`) vs Option B (server-initiated `ServerPing`/`ServerPong`)
- Establishes trust model: client-reported metrics are diagnostics only; server-initiated ping is authoritative for enforcement
- Recommends Option B (server-initiated) for future authoritative disconnect enforcement
- Maps future milestone path: M4.16 client-reported health (diagnostic), M4.17 server-initiated ping (authoritative enforcement)
- No protocol wire changes, no live code changes, no enforcement
- New doc: `docs/heartbeat-authority-design.md`

---

## Milestone 4.14

Strict heartbeat enforcement wiring:

- `ClientHeartbeatPolicy` enum (`ReportOnly`, `Strict`) added to `client/src/lib.rs`
- `CLIENT_HEARTBEAT_DISCONNECT_THRESHOLD = 3` — matches server-side `MISSED_HEARTBEAT_DISCONNECT_THRESHOLD`
- `heartbeat_loop` gains `policy: ClientHeartbeatPolicy` parameter; breaks with `enforcement_disconnect=true` when `Strict` + threshold reached
- `HeartbeatMetrics.enforcement_disconnect: bool` — set when enforcement triggered the loop exit
- `to_text()` appends `heartbeat_enforcement_disconnect: true` when set
- `--heartbeat-policy strict|report_only` CLI flag for the client (default: `report_only`)
- Client logs enforcement disconnect message when triggered
- 4 client enforcement tests (`heartbeat_enforcement.rs`): ReportOnly no-disconnect, Strict disconnects at threshold, Strict stays connected when healthy, metrics correct
- 3 server heartbeat enforcement tests (`heartbeat_enforcement.rs`): registry cleanup after disconnect, ReportOnly session stays connected, Strict session stays connected when healthy
- Server enforcement (heartbeat): no change — server view always has `timeout_or_error=0`; all per-session labels remain observational. Enforcement is a client-side decision.
- No protocol wire changes; no Login capability changes

---

## Milestone 4.13

Heartbeat policy config plumbing:

- `HeartbeatPolicy` gains `#[derive(Deserialize)]` + `#[serde(rename_all = "snake_case")]`
- `HeartbeatSection { policy: HeartbeatPolicy }` added to `config.rs`; defaults to `ReportOnly`
- `ServerConfig` has `pub heartbeat: HeartbeatSection`
- `to_lifecycle_summary_text()` includes `heartbeat_policy:` line
- `SessionRegistry` and `SessionRegistrySnapshot` carry `heartbeat_policy: HeartbeatPolicy`
- `set_heartbeat_policy()` on `SessionRegistry`; called once at startup from `run_with_listener_and_state()`
- `to_diagnostics_text()` evaluates `heartbeat=<label>` under configured policy (not hardcoded `ReportOnly`)
- All `.with_heartbeat_policy()` calls in `lib.rs` use `&config.heartbeat.policy`
- `ServerRuntimeStatus` has `heartbeat_policy: String`; surfaced in `to_text()` and admin `status`
- 7 config tests, 5 registry policy tests, 2 admin policy tests, 3 status tests
- `example.server.toml` gains `[heartbeat]` section with comments
- No disconnect enforcement, no protocol changes

---

## Milestone 3.0

Session enforcement dry-run:

- New `crates/server/src/enforcement.rs` — `SessionEnforcementPolicy` (ReportOnly, Strict),
  `SessionEnforcementDecision` (Allow, WouldDisconnectInvalidFirstMessage,
  WouldDisconnectVersionMismatch, WouldDisconnectCapabilityGateFailure,
  WouldDisconnectInvalidStateTransition, WouldMarkSessionFailed),
  `evaluate_enforcement()` pure function, `Decision::to_text()` display
- 21 unit tests: report-only always allows, strict allows ReadyDryRun,
  strict disconnects Connected/no-progress, strict disconnects version
  mismatch, strict marks failed for generic failure or intermediate state,
  deterministic text output, version extraction from reason string, policy
  equality
- No live behavior change — decisions are pure values, not executed
- No protocol wire changes, no config changes, no new dependencies
- `docs/session-enforcement-dry-run.md` — new doc

## Milestone 3.1

Server lifecycle config cleanup:

- `ServerConfig::to_lifecycle_summary_text()` — deterministic multi-line text
  covering server identity, protocol policy flags, resource dir, join gate mode,
  diagnostics settings, admin mode, logging level/format
- Logged at startup via `info!("server lifecycle config:\n{}", ...)` alongside
  the existing structured log line
- 6 new config unit tests (deterministic, includes dry-run policies, includes
  admin/logging/diagnostics, includes server identity, reflects admin enabled,
  no IP/personal data)
- All existing validation rules unchanged and still enforced
- `docs/server-config.md` updated with lifecycle summary documentation

## Milestone 3.2

Server lifecycle smoke tests:

- New `tests/smoke.rs` integration test file — 8 tests, no flaky networking,
  no tokio runtime required
- Tests: example config file validates, default config initializes lifecycle,
  runtime status from config + empty registry, shutdown summary from default
  lifecycle, admin stdin disabled by default, no remote admin fields,
  lifecycle summary has no IP/personal data, dry-run policies reflected in
  summary
- `server` crate exports `ShutdownReason`, `ShutdownSummary`,
  `build_shutdown_summary` for integration test use
- All existing validation rules unchanged and still enforced
- No downloads, execution, remote admin, persistence, or telemetry

## Milestone 3.3

Resource download design spec (docs-only):

- `docs/resource-download-design.md` — comprehensive design specification
- Scope: future download and cache repair, no current implementation
- Threat model: malicious server, corrupted cache, partial downloads, path
  traversal, symlink attacks, hash mismatch, replay, unsigned announcements,
  archive risks
- Allowed future behaviour: staged downloads, verify-before-commit, no
  execution, explicit cache repair only
- Disallowed behaviour: no auto-execution, no system-path writes, no symlink
  following, no unsigned enforcement, no GTA integration
- Proposed future DTOs: `ResourceDownloadRequest`, `ResourceDownloadOffer`,
  `ResourceFileChunk`, `ResourceDownloadComplete`, `ResourceDownloadError`
  (design only, not implemented)
- Staging/cache model: staging dir, `.partial` naming, atomic rename,
  verify-before-commit, cleanup on failure, deterministic layout
- Signature relationship: downloads require signed announcements; signature
  enforcement gated behind its own milestone
- Join gate relationship: remains dry-run until download/repair/signature
  model is mature
- 8 open questions documented (chunk size, compression, archive support,
  retry policy, signed index format, trust roots, cache eviction, offline
  mode)
- Next recommended milestones shifted: M3.4 (cache repair plan), M3.5
  (signature verification design), M3.6 (signature DTO refinement), M3.7
  (Ed25519 verification), M3.8 (trusted key config), M3.9 (download DTOs),
  M3.10 (staging model)
- No source code modified. No dependencies added. No network endpoints.

## Milestone 3.4

Resource cache repair planning (report-only):

- `CacheRepairAction` enum: `None`, `FetchMissing`, `ReplaceInvalid`, `VerifyOnly`
- `CacheRepairPlanEntry` — per-file entry with action derived from `CacheFileStatus`
- `CacheRepairPlan` — aggregate plan with counts and `is_noop()` / `to_text()`
- `build_cache_repair_plan(&CacheVerificationReport) -> CacheRepairPlan` — pure function, no I/O
- Mapping: `Valid→None`, `Missing→FetchMissing`, `SizeMismatch/HashMismatch→ReplaceInvalid`
- Client `--plan-cache-repair <resource_dir> <cache_dir>` — inspect-only CLI flag
- 9 new unit tests: all-valid→noop, missing→FetchMissing, size mismatch→ReplaceInvalid, hash mismatch→ReplaceInvalid, mixed counts, deterministic ordering, text output, fully valid text, no filesystem access
- No downloads, no file writes, no execution, no network access
- `docs/resource-cache-repair-plan.md`
- All existing boundaries preserved

## Milestone 3.5

Real signature verification design (docs-only):

- `docs/signature-verification-design.md` — comprehensive design specification
- Scope: future-only, no crypto, no enforcement, no implementation
- Current state documented: `ResourceAnnouncementSignature` metadata stub,
  `check_announcement_signature_stub` returns `NotProvided`/`NotChecked`
- Canonical signing target defined: protocol version, resource name/version,
  requirement level, file list with paths/sizes/hashes
- Canonicalization requirements: stable field order, UTF-8, normalized relative
  paths, no absolute paths, no `..`, no symlinks, no non-deterministic maps,
  no optional timestamps unless explicitly included
- Algorithm recommendation: Ed25519 (RFC 8032) with algorithm agility via
  existing `algorithm` field; no custom crypto
- Trust model: per-server pinned keys, TOFU as secondary option, `key_id`
  dispatch, key rotation, key revocation (future), no global trust anchors
- Verification flow: canonicalize → look up key → verify → report
- 11 failure modes documented: no signature, unsupported algorithm, unknown
  key_id, invalid signature, replay, canonicalization mismatch, etc.
- Enforcement policy: report-only first, phased activation via config gating,
  never enforced before explicit milestone
- Relationship to download design: signature verifies metadata, hash verifies
  content, no-exec boundary is separate
- 12 open questions with recommendations (JSON vs CBOR, key storage format,
  key rotation UX, revocation mechanism, validity timestamps, offline mode,
  server browser trust metadata, multi-resource bundle signatures,
  per-resource vs per-announcement signatures, signature format encoding,
  timestamp source, key ID format)
- Next milestones: M3.6 (signature DTO refinement), M3.7 (Ed25519 impl),
  M3.8 (trusted key config), M3.9 (download DTOs), M3.10 (staging model)
- No source code modified. No dependencies added. No network endpoints.
- `docs/signed-resource-announcements.md` updated to cross-reference design
- `docs/resource-download-design.md` updated with new milestone numbering
- `docs/security-boundaries.md` updated with cross-reference

## Milestone 3.6

Signature metadata model:

- `SignatureAlgorithm` enum with `Ed25519` variant, `Display`/`FromStr`, `known_names()`, `is_known()`
- `SignatureMetadataError` enum: `EmptyAlgorithm`, `EmptyKeyId`, `EmptySignature`, `UnsupportedAlgorithm(String)`
- `validate_signature_metadata(&ResourceAnnouncementSignature) -> Result<(), SignatureMetadataError>` — checks non-empty algorithm/key_id/signature, known algorithm
- `CanonicalAnnouncementPayload`, `CanonicalResourcePayload`, `CanonicalFilePayload` — typed DTOs defining what would be signed
- `build_canonical_payload(&ResourceAnnouncement) -> Option<CanonicalAnnouncementPayload>` — deterministic canonical form: resources sorted by name, files sorted by relative_path; returns `None` when signature absent or algorithm/key_id empty
- `check_announcement_signature_stub` updated: maps `UnsupportedAlgorithm` → `UnsupportedAlgorithm` status, other validation errors → `Invalid`, well-formed metadata → `NotChecked`
- 21 new protocol unit tests (60 total)
- No crypto dependencies, no cryptographic verification, no base64 validation
- `docs/signature-metadata-model.md` — new doc

## Milestone 3.7

Signature verification dry-run planner:

- `TrustedKey` struct — key identity (key_id + algorithm), no crypto material
- `SignatureVerificationAction` enum: `VerifySignature`, `MissingSignature`, `UnsupportedAlgorithm`, `UnknownKeyId`, `MalformedSignature`, `WouldRejectUnsigned`, `ResourceDigestMismatchPrecheck` — with `Display` impl
- `SignatureVerificationPlanEntry` — per-resource entry with resource_name, action, reason
- `SignatureVerificationPlan` — aggregate plan, `is_empty()`, `to_text()` (deterministic, sorted by resource name)
- `build_signature_verification_plan(&ResourceAnnouncement, &[TrustedKey], reject_unsigned: bool) -> SignatureVerificationPlan` — pure function, no I/O, no crypto
- Logic per resource: no signature → MissingSignature or WouldRejectUnsigned (by policy); metadata invalid → MalformedSignature; unsupported alg → UnsupportedAlgorithm; key_id untrusted → UnknownKeyId; all good → VerifySignature
- 16 new protocol unit tests (76 total)
- No crypto dependencies, no cryptographic verification, no downloads, no enforcement
- `docs/signature-verification-dry-run.md` — new doc

## Milestone 3.8

Signature verification engine (real Ed25519 crypto):

- `TrustedPublicKey` struct — key_id + algorithm + raw public key bytes
- `VerificationError` enum — `UnsupportedAlgorithm`, `UnknownKeyId`, `InvalidSignature`, `MalformedKeyMaterial`, `MalformedSignatureBytes`, `SignedPayloadMismatch`, `CanonicalPayloadMissing`
- `VerificationOutcome` enum — `Valid`, `Invalid { error }`, `Skipped { reason }`
- `VerificationEntry` — per-resource entry with resource_name + outcome
- `VerificationReport` — aggregate report, `is_empty()`, `all_valid()`, `to_text()`
- `verify_ed25519_signature(payload_bytes, signature_b64, public_key_bytes)` — pure Ed25519 verification using `ed25519-dalek`
- `execute_verification_plan(&ResourceAnnouncement, &SignatureVerificationPlan, &[TrustedPublicKey]) -> VerificationReport` — consumes M3.7 plan, produces real verification results
- All plan entries receive the same announcement-level outcome (announcement-level signature covers all resources equally)
- `crates/protocol/src/signature_engine.rs` — new module in protocol crate
- Dependencies: `ed25519-dalek` v2.2, `base64` v0.22
- 14 new signature engine unit tests (90 protocol tests total)
- No enforcement, no disconnects, no downloads, no cache writes
- `docs/signature-verification-engine.md` — new doc

## Milestone 3.9

Wire signature verification into resource flow (report-only):

- Client gains `--verify-announcement-signature <path>` CLI flag — offline inspection of a JSON `ResourceAnnouncement` with optional `--trusted-keys <path>` and `--reject-unsigned` flags
- Output shows both the M3.7 verification plan and the M3.8 engine report
- Live `ResourceAnnouncement` handler uses engine when `trusted_keys_file` is configured in client config; otherwise falls back to stub
- `TrustedKeyEntry` deserialization from TOML — `key_id`, `algorithm`, `public_key_b64`
- `load_trusted_keys(path) -> Result<Vec<TrustedPublicKey>>` — pure I/O helper
- `MEOWV_TRUSTED_KEYS` env var support
- 5 new end-to-end protocol tests (95 protocol tests total)
- 300 workspace tests passing
- No enforcement, no disconnects, no downloads, no cache writes
- `docs/signature-verification-reporting.md` — new doc

## Milestone 4.12

Heartbeat policy report in admin/status:

- `HeartbeatDecision::to_short_label()` — concise `&'static str` for compact output: `no_activity`, `healthy`, `no_pong_yet`, `warn_timeout`, `unhealthy`, `would_disconnect`
- `SessionRegistrySnapshot::to_diagnostics_text()` — per-session line now appends `heartbeat=<label>` derived from `ping_received_count`/`pong_sent_count` via `evaluate_heartbeat(ReportOnly)`
- Admin `sessions` and `diagnostics` commands show heartbeat health label without extra commands or flags
- 5 new `session_registry` tests + 2 new `admin` tests + 6 new `heartbeat_planner` `to_short_label` tests
- No enforcement, no config change, no protocol change
- `docs/server-admin-debug-commands.md` updated with sample output
- `docs/roadmap.md` updated

---

## Milestone 4.11

Heartbeat timeout policy planner:

- `HeartbeatPolicy` enum: `ReportOnly` / `Strict`
- `HeartbeatPlannerInput` struct: `ping_sent`, `pong_received`, `timeout_or_error` — all cumulative counts
- `HeartbeatDecision` variants: `NoHeartbeatObserved`, `Healthy`, `WouldWarnNoPongYet`, `WouldWarnTimeout`, `WouldMarkUnhealthy`, `WouldDisconnectMissedHeartbeat`
- `evaluate_heartbeat()` — pure deterministic evaluator; `MISSED_HEARTBEAT_DISCONNECT_THRESHOLD = 3`
- `HeartbeatDecision::to_text()` — deterministic human-readable output
- `SessionDiagnostics::with_heartbeat_policy()` — builder chains heartbeat decision into diagnostics text and JSON output
- All `SessionDiagnostics` build sites in `handle_client` chain `with_heartbeat_policy(ReportOnly)` — decision appears in diagnostic logs
- Server-only view: `timeout_or_error = 0` (server does not receive client-side timeout counts)
- Under `ReportOnly`: never escalates to `WouldDisconnectMissedHeartbeat`
- 20 new heartbeat planner unit tests
- No actual disconnect, no enforcement, no protocol change, no config change
- `docs/client-heartbeat.md` updated with policy planner section
- `docs/roadmap.md` updated

---

## Milestone 4.10

Heartbeat admin observability:

- `SessionRegistryEntry` gains `ping_received_count` and `pong_sent_count` fields (derived from event log when registry is updated)
- `SessionRegistry::update_session_heartbeat_counts()` — push counts to registry entry after each Pong is sent
- `SessionRegistrySnapshot::to_diagnostics_text()` — per-session line now includes `ping_rx=N  pong_tx=N`
- `handle_client` in `lib.rs` updates registry heartbeat counts after each Pong send
- Admin `sessions` and `diagnostics` commands show heartbeat counts without additional commands or flags
- Zero counts shown explicitly when no heartbeat activity has occurred
- 5 new `session_registry` unit tests + 3 new `admin` unit tests
- No new commands, no enforcement, no protocol change, no duplicate counter state
- `docs/server-admin-debug-commands.md` updated — sessions output example and command description
- `docs/client-heartbeat.md` updated — server-side observability section
- `docs/roadmap.md` updated with M4.9 and M4.10 entries

---

## Milestone 4.0

Strict signature enforcement gate:

- `SignaturePolicy` enum: `ReportOnly` (default) / `Strict`
- `SignaturePolicyViolation` struct with `message: String`
- `evaluate_signature_policy(&VerificationReport, &SignaturePolicy) -> Result<(), SignaturePolicyViolation>` — pure function
- Strict policy rejects when engine report is not `all_valid()`: unsigned announcements, invalid signatures, unknown key IDs, unsupported algorithms
- ReportOnly policy never rejects (returns `Ok`)
- Client `--signature-policy strict` CLI flag + `SignaturePolicy` used in both offline and live paths
- Live path under strict: violation → error print + break out of read loop (no availability report sent)
- CLI path under strict: violation → error print + `anyhow::bail!`
- 7 new protocol unit tests (102 protocol tests total)
- 307 workspace tests passing
- No downloads, no cache writes, no execution, no silent fallback
- `docs/strict-signature-policy.md` — new doc

## Milestone 4.1

Trust key UX + policy validation:

- `KeyConfigError` enum: `EmptyConfig`, `DuplicateKeyId`, `UnsupportedAlgorithm`, `MalformedKeyMaterial` — with Display
- `validate_trusted_key_config(&[TrustedPublicKey]) -> Result<(), KeyConfigError>` — rejects empty, duplicate key IDs, unsupported algorithms, wrong key material length
- Client CLI validates loaded keys with `validate_trusted_key_config`
- `print_trusted_keys_summary` — prints count + key IDs
- `--signature-policy strict` requires `--trusted-keys <path>` or fails with clear error
- `--signature-policy report-only` with no trusted keys prints informational warning
- 6 new protocol unit tests (108 protocol tests total)
- 313 workspace tests passing
- No downloads, no cache writes, no execution, no silent downgrade
- `docs/strict-signature-policy.md` updated

## Milestone 4.3

Policy config sections + UX:

- `SignatureSection` added to server config with `policy: SignaturePolicy` field (default `ReportOnly`)
- `SignaturePolicy` in protocol crate gains `Serialize, Deserialize` derives with `#[serde(rename_all = "snake_case")]`
- Lifecycle summary now includes `signature_policy: report_only` / `strict`
- `ServerRuntimeStatus` gains `session_enforcement` and `signature_policy` string fields, visible in admin `status` output
- `example.server.toml` updated with `[enforcement]` and `[signature]` sections with inline comments
- 3 new config unit tests: lifecycle summary includes both policies, strict enforcement in lifecycle, strict signature in lifecycle
- 354 total workspace tests passing (was 351)
- No enforcement behavior changes — purely config organization and visibility
- `docs/server-policy-configuration.md` — new doc covering both sections

---

## Milestone 4.2

Wire session enforcement into `handle_client` (live behavioral change):

- `handle_enforcement()` helper function added to `crates/server/src/lib.rs` — evaluates enforcement decision and disconnects under `Strict` policy; under `ReportOnly` logs enforcement context in diagnostics
- `map_decision_to_disconnect()` helper maps `SessionEnforcementDecision` variants to `(DisconnectReason, String)`
- 6 soft-failure transition points in `handle_client` now trigger enforcement under `Strict`:
  - `on_negotiation_logged()` failure → fail session + disconnect with reason
  - `on_resource_announcement_sent()` failure → fail session + disconnect
  - `on_availability_report_received()` failure → fail session + disconnect
  - `on_policy_evaluated()` non-blocked error → fail session + disconnect
  - `on_join_gate_sent()` failure → fail session + disconnect
  - `mark_ready_dry_run()` failure → fail session + disconnect
- Pre-writer-task enforcement points use `send_direct()` for immediate Disconnect write
- Post-writer-task enforcement points use `client_tx.send()` + `break` for channel-based Disconnect delivery
- Existing hard-failure paths (non-Login first message, version mismatch, hello failure) unchanged in behavior, but diagnostics now include `.with_enforcement()` context
- `ReadyDryRun` diagnostics also include enforcement context showing policy and Allow decision
- All enforcement actions: record `Failed` event, update registry to `Failed`, print diagnostics with enforcement context, send `Disconnect`, return from handler (or break to normal cleanup)
- 5 new integration tests in `tests/session_enforcement.rs`:
  - `report_only_successful_handshake_reaches_ready_dry_run` — baseline: ReportOnly handshake succeeds
  - `strict_successful_handshake_reaches_ready_dry_run` — Strict does not break successful path
  - `strict_version_mismatch_disconnects` — Strict disconnects on version mismatch with `ProtocolMismatch`
  - `strict_invalid_first_message_disconnects` — Strict disconnects on non-Login first message
  - `strict_handshake_cleans_up_registry_on_disconnect` — registry cleaned after enforcement disconnect
- Registry cleanup preserved via existing `SessionGuard` RAII
- No protocol wire changes, no config redesign, no capability field changes
- 351 total workspace tests passing (was 346)
- `docs/live-session-enforcement.md` — new doc

---

Standalone prototype:

- Rust workspace
- Nix dev shell
- shared protocol
- server
- dummy client
- login/chat/entity sync

## Milestone 1

Protocol hardening:

- compatibility policy docs
- heartbeat/ping
- better config files
- integration tests

## Milestone 1.8

Protocol compatibility negotiation design:

- dry-run negotiation data structures
- version ranges, capability flags
- intersection/evaluation logic
- server/client dry-run reporting
- docs explaining future activation path

## Milestone 2.9

Admin sessions command — live per-session state from registry:

- `sessions` admin command now uses registry snapshot for per-session details:
  session ID, state, event count, protocol_version, ready_dry_run, failed
- `SessionRegistryEntry` gains `protocol_version: Option<u32>` — set after
  version check in `handle_client`
- `SessionRegistry::set_protocol_version()` — new method
- `SessionRegistrySnapshot::to_diagnostics_text()` extended with protocol version
- `admin_stdin_loop` passes registry snapshot to context handler so sessions
  command gets live registry data; falls back to status counts when registry
  unavailable
- 5 new admin unit tests: empty registry, connected session, ReadyDryRun with
  protocol version, fallback to counts, registry preferred over status
- `docs/server-admin-debug-commands.md` updated with sessions output format
- No protocol wire changes, no enforcement, read-only

## Milestone 2.8

Full handshake integration assertions:

- New `crates/server/tests/handshake_observability.rs` — 7 integration tests
- `SharedState` and `ClientInfo` made pub for test inspection
- `run_with_listener_and_state()` — test helper exposing Arc<SharedState> for registry observation
- Tests cover:
  - `full_handshake_creates_session_and_reaches_ready_dry_run`: asserts session created, state=ReadyDryRun, event_count=11, protocol_version set, registry cleaned after disconnect, runtime status matches
  - `version_mismatch_disconnects_and_cleans_up_session`: asserts Disconnect + session cleanup
  - `invalid_handshake_first_message_not_login`: asserts InvalidHandshake disconnect + cleanup
  - `registry_session_id_is_deterministic`: asserts deterministic session-1 ID, protocol_version
  - `session_created_on_connect_before_login`: session exists in Connected before any message
  - `session_cleaned_up_on_early_disconnect`: session created then cleaned on connection drop
  - `runtime_status_reflects_live_session_counts`: status snapshot matches registry across multiple connections
- 7 new integration tests, 325 workspace tests passing
- No behavior changes, no protocol changes, no wire format changes

## Milestone 2.7

Live session registry:

- `SessionId` — monotonic u64 newtype, Copy, Display, never IP-based, deterministic in tests
- `SessionRegistryEntry` — id, state, event_count, ready_dry_run, failed; no personal data
- `SessionRegistrySnapshot` — aggregate counts + deterministic ordered session list
- `SessionRegistry` — BTreeMap-backed, create/update/remove/snapshot API
- `SessionGuard` RAII guard — removes session on drop, covers all `handle_client` exit paths
- `SharedState` gains `registry: Arc<Mutex<SessionRegistry>>`
- `handle_client` creates session at connect, updates state+event_count at every transition
- `admin_stdin_loop` takes `Arc<SharedState>`, rebuilds live status per command
- Admin `status` and `sessions` commands show real session counts
- 12 session registry unit tests
- `docs/session-registry.md`

## Milestone 2.6

Server runtime status snapshot:

- `ServerRuntimeStatus` — 13 fields: server identity, protocol policy flags,
  session counts (default 0), resource dir, diagnostics/admin flags
- `from_config(&ServerConfig)` — derives snapshot from config; no timestamps
- `with_session_counts(connected, ready_dry_run, failed)` — returns updated snapshot
- `to_text()` — deterministic `key: value` multi-line output; no client IPs
- `handle_admin_command_with_status(command, Option<&ServerRuntimeStatus>)` — status/sessions/resources commands use snapshot data when provided
- `handle_admin_command` now delegates to `handle_admin_command_with_status(cmd, None)`
- `admin_stdin_loop` accepts `ServerConfig`, builds snapshot at startup, uses status-aware handler
- `docs/server-runtime-status.md`
- 8 status unit tests; 4 admin integration tests

## Milestone 2.5

Local server admin debug commands:

- `AdminCommand` enum (Help, Status, Sessions, Resources, Diagnostics, Quit)
- `AdminCommandParseError` (Empty, UnknownCommand) with Display + Error impls
- `AdminCommandResult` { command, message, should_quit }
- `parse_admin_command` — case-insensitive, whitespace-trimmed, 6 commands
- `handle_admin_command` — placeholder messages; `should_quit=true` for Quit only
- `AdminSection { local_stdin_enabled: bool }` added to `ServerConfig`
- `run_with_listener` refactored into `accept_loop` + `admin_stdin_loop`
- stdin loop gated on `config.admin.local_stdin_enabled`; oneshot channel signals quit
- `example.server.toml` gains `[admin]` section
- 12 admin parser unit tests; 3 AdminSection config tests
- `docs/server-admin-debug-commands.md`

## Milestone 2.4

Structured logging / tracing config:

- `LogLevel` enum (Trace/Debug/Info/Warn/Error), `LogFormat` enum (Text/Json),
  `LoggingSection` in server config
- `[logging]` section in `example.server.toml`
- `init_logging(&LoggingSection)` — branches on format (text/json), applies
  level and show_targets, uses `try_init()` to avoid double-init panics
- `RUST_LOG` env var still takes precedence over config level
- "logging initialized" info line emitted at startup
- main.rs loads config before calling init_logging
- Server already uses `info!/warn!/error!` throughout; no println! present
- 6 new logging config unit tests (all levels parse, invalid level/format
  rejected at parse time, default validates, JSON format parses)

## Milestone 2.3

Server config for dry-run policies:

- `crates/server/src/config.rs` — structured `ServerConfig` with five sections
  (server, protocol, resources, join_gate, diagnostics)
- `ServerConfig::load_from_path()`, `load_with_env()`, `validate()`
- Validation rejects unsafe settings: `exact_version_required=false`,
  `negotiation_dry_run=false`, `enforce_required_resources=true`, path traversal
- `DiagnosticsFormat` and `JoinGateConfigMode` enums with serde deserialization
- Server binary gains `--config <path>` CLI flag
- `MEOWV_SERVER_BIND` and `MEOWV_TICK_RATE` env overrides preserved
- `example.server.toml` updated to new sectioned format
- Diagnostics prints gated on `diagnostics.print_session_diagnostics`
- Diagnostics format switchable (`text` / `json_stub`) via config
- 12 config unit tests

## Milestone 2.2

Session diagnostics / debug dump:

- `SessionDiagnostics` struct collecting current state, history, event log,
  last event message, ready_dry_run flag, failure reason
- `from_parts(&SessionStateMachine, &SessionEventLog) -> Self` — read-only snapshot
- `to_text()` — deterministic human-readable multi-line output
- `to_json_stub()` — manually-formatted JSON, no serde dependency added
- printed to server info log at ReadyDryRun and Failed (version mismatch)
- in-memory only; no persistence, no network exposure, no IP/personal data
- 8 unit tests

## Milestone 2.1

Session event log / audit trail:

- in-memory `SessionEventLog` with `SessionEventKind` variants
- `SessionEvent` carrying sequence, kind, state, and message (no timestamps)
- integrated into `handle_client` alongside every state transition
- records Connected, HelloReceived, VersionChecked, ProtocolNegotiationDryRun,
  CapabilityGateChecked (×2), ResourceAnnouncementSent, AvailabilityReportReceived,
  ResourcePolicyEvaluated, JoinGateDryRunSent, ReadyDryRun, and Failed
- session audit summary logged at session end
- 8 unit tests; no timestamps, no IP/personal data, no persistence

## Milestone 2.0

Server session state machine:

- explicit `SessionState` enum (Connected → … → ReadyDryRun / Failed)
- `SessionStateMachine` with forward-only transitions, terminal Failed state
- `SessionStateError` variants (InvalidTransition, ProtocolMismatch, PolicyBlockedDryRun, …)
- server `handle_client` tracks state through full handshake pipeline
- each transition logged at `info` level; no enforcement changes
- docs explaining state graph, dry-run nature, and future enforcement point

## Milestone 1.9

Protocol capability-gated resource flow:

- capability gate helpers (`profile_supports_capability`, `shared_capabilities`,
  `requires_capability`, `capability_gate_report`)
- server logs capability gate before `ResourceAnnouncement` and `JoinGateDecision`
- client prints local capabilities on connect and `--protocol-negotiation`
- report-only: no enforcement, no disconnects, no behaviour change
- docs explaining gate model and future activation path

## Milestone 1.0

Resource registry:

- multi-resource discovery
- dependency validation
- cycle detection
- deterministic load order

## Milestone 1.1

Runtime boundary:

- no-exec load planning
- runtime separation prep
- deterministic resource planning

## Milestone 1.2

Runtime state machine:

- no-exec lifecycle simulation
- deterministic resource states
- dependency readiness checks

## Milestone 0.5

Game edition layer:

- edition-aware metadata types
- conservative placeholder detection
- clean-room support policy docs
- no runtime GTA V integration


## Milestone 2

Resource/runtime model:

- server resource manifest format
- script/runtime abstraction
- permission model
- hot-reload experiments in standalone environment

## Milestone 3 (Longer-Term)

Transport/runtime refinement:

- snapshot interpolation experiments
- reliability channels
- interest management prototype
- metrics and tracing exports

## Milestone 4

Native boundary evaluation:

- decide whether any low-level bridge is necessary
- if yes, isolate in narrow crate/module
- require legal and architectural review before implementation
