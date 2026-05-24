# Server Policy Configuration (Milestones 4.3, 4.13)

## Overview

The server has three configurable policy sections that control how
enforcement decisions are applied:

| Section | Field | Values | Default |
|---------|-------|--------|---------|
| `[enforcement]` | `mode` | `"report_only"`, `"strict"` | `"report_only"` |
| `[signature]` | `policy` | `"report_only"`, `"strict"` | `"report_only"` |
| `[heartbeat]` | `policy` | `"report_only"`, `"strict"` | `"report_only"` |

## Session Enforcement Policy (`[enforcement]`)

Controls how session handshake failures are handled in `handle_client`.

| Mode | Behavior |
|------|----------|
| `"report_only"` | Enforcement decisions are logged in diagnostics but never acted upon. Existing hard-failure paths (non-Login first message, version mismatch) still disconnect as before. |
| `"strict"` | Invalid handshake transitions, version mismatches, capability gate failures, and other session errors cause a clean disconnect with a structured reason. Successful handshakes reach `ReadyDryRun` as normal. |

See `docs/live-session-enforcement.md` for detailed behavior.

## Signature Policy (`[signature]`)

Controls how resource announcement signature verification is handled.
This is a server-side configuration for future use; signature enforcement
is currently client-side only.

| Mode | Behavior |
|------|----------|
| `"report_only"` | Signature verification status is reported but never causes rejection. |
| `"strict"` | Unsigned or invalid signature verification causes the resource announcement to be rejected. |

## Heartbeat Policy (`[heartbeat]`)

Controls how the heartbeat planner decision is evaluated per session.

| Mode | Behavior |
|------|----------|
| `"report_only"` | Heartbeat labels (`heartbeat=<label>`, `srv_heartbeat=<label>`) computed and surfaced in diagnostics/admin output. No disconnect occurs regardless of miss count. |
| `"strict"` | Server-initiated direction: when missed server pongs ≥ `MISSED_SERVER_PONG_DISCONNECT_THRESHOLD (3)`, the session is failed and the TCP connection is closed. Client-initiated direction labels are observational only (server has no `timeout_or_error` data). |

The policy is set at startup via `set_heartbeat_policy()` on the session registry.
All per-session `heartbeat=<label>` and `srv_heartbeat=<label>` values in admin
`sessions` and `diagnostics` output reflect the configured policy.

See `docs/client-heartbeat.md` for planner decisions, label descriptions, and
enforcement invariants. See `docs/m4-enforcement-heartbeat-summary.md` for the
full M4 stack summary.

## Example Config

### ReportOnly (default, safe)

```toml
[enforcement]
mode = "report_only"

[signature]
policy = "report_only"

[heartbeat]
policy = "report_only"
```

### Strict Session Enforcement

```toml
[enforcement]
mode = "strict"

[signature]
policy = "report_only"

[heartbeat]
policy = "report_only"
```

### Strict Everything

```toml
[enforcement]
mode = "strict"

[signature]
policy = "strict"

[heartbeat]
policy = "strict"
```

## Validation

Both policy fields are deserialized from TOML via serde. Unknown enum
values (e.g. `mode = "permissive"`) produce a clear parse error at
startup.

There is no silent downgrade: if `"strict"` is explicitly configured,
the server uses `Strict` policy. The lifecycle summary logged at
startup includes both policy values for operator inspection.

## Visibility

Policies are visible in:

- **Startup log**: `info!("server lifecycle config:\n{}", ...)` includes
  `session_enforcement: report_only` / `strict` and
  `signature_policy: report_only` / `strict`
- **Admin status command**: `ServerRuntimeStatus::to_text()` includes
  `session_enforcement`, `signature_policy`, and `heartbeat_policy` fields
- **Session diagnostics**: When the session fails and `print_session_diagnostics`
  is enabled, the diagnostic output includes the active enforcement policy
  and the decision that was evaluated

## Hard Boundaries

- No protocol wire-format changes
- No new enforcement behavior beyond M4.2
- No resource download/cache changes
- No execution of resources
- No silent fallback from Strict to ReportOnly
