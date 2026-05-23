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
| 3.0 | Architecture refresh — crate/module map, pipeline docs, security boundaries |
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

---

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

---

## Milestone 0

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
