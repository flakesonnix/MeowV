# Security Boundaries

## Operational Security

MeowV is a local-only, dry-run, standalone prototype. It enforces no runtime
policy and exposes no network-accessible management interface. This document
captures the operational security boundaries that hold across all current and
planned milestones.

## What the System Does NOT Do

### No Game Integration
- No GTA V process interaction.
- No memory reading or writing.
- No function hooking or injection.
- No anti-cheat bypass.
- No DRM bypass.
- No Rockstar service bypass.

### No Remote Access
- No remote admin API.
- No web panel.
- No network-accessible debug interface.
- No authentication portal.
- No remote command execution.

### No Downloads or Execution
- No file downloads (even in future design, downloads are spec-only).
- No file serving.
- No script execution (Lua, JS, WASM, or custom bytecode).
- No scripting runtime.
- No arbitrary command execution.

### No Persistence or Telemetry
- No database writes.
- No file writes beyond optional config reads.
- No telemetry export.
- No metrics aggregation.
- No log shipping.
- All session/registry/diagnostics data is in-memory and ephemeral.

### No Personally Identifiable Information
- No IP addresses in diagnostics, status, registry, or shutdown output.
- No player names in registry entries.
- No timestamps in event logs or diagnostics.
- Session IDs are opaque monotonic integers, not IP-based.
- No tokens, passwords, or credentials are stored or emitted.

## What the System Does

- Accepts local TCP connections from a dummy CLI client.
- Runs a config-driven handshake pipeline in dry-run mode.
- Logs structured diagnostics to stdout via `tracing`.
- Provides local stdin admin commands (gated by config, disabled by default).
- Tracks session state and events in memory (per-task event log + shared
  registry).
- Produces deterministic text dumps for status, diagnostics, and shutdown
  summaries.
- Shuts down gracefully on local admin `quit` command.

## Enforcement Model

All policy decisions are currently dry-run or report-only:

| Gate | Current Behaviour |
|---|---|
| Protocol version | Enforced — mismatch causes disconnect |
| Protocol negotiation | Not enforced — logged only |
| Join gate | Not enforced — logged only |
| Capability requirements | Not enforced — logged only |
| Resource compatibility | Not enforced — reported only |
| Announcement signatures | Not verified — stub only |

A future milestone may activate enforcement for specific gates, but each
activation will require its own milestone, explicit config gating, and a
security review.

## Config Safety

The `ServerConfig` validator rejects:
- `exact_version_required = false` — protocol version matching must stay
  enforced.
- `negotiation_dry_run = false` — negotiation stays dry-run.
- `enforce_required_resources = true` — resource policy stays report-only.
- Path traversal in resource or cache directories.
- Unparseable bind addresses.

These checks are a hard error at startup and cannot be bypassed at runtime.

## Related Documents

- `docs/legal-boundaries.md` — legal/compliance boundaries (proprietary code,
  clean-room requirements, risk notes).
- `docs/architecture.md` — crate/module map, pipelines, dry-run policies.
