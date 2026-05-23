# Server Policy Configuration (Milestone 4.3)

## Overview

The server has two configurable policy sections that control how
enforcement decisions are applied:

| Section | Field | Values | Default |
|---------|-------|--------|---------|
| `[enforcement]` | `mode` | `"report_only"`, `"strict"` | `"report_only"` |
| `[signature]` | `policy` | `"report_only"`, `"strict"` | `"report_only"` |

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

## Example Config

### ReportOnly (default, safe)

```toml
[enforcement]
mode = "report_only"

[signature]
policy = "report_only"
```

### Strict Session Enforcement

```toml
[enforcement]
mode = "strict"

[signature]
policy = "report_only"
```

### Strict Everything

```toml
[enforcement]
mode = "strict"

[signature]
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
  `session_enforcement` and `signature_policy` fields
- **Session diagnostics**: When the session fails and `print_session_diagnostics`
  is enabled, the diagnostic output includes the active enforcement policy
  and the decision that was evaluated

## Hard Boundaries

- No protocol wire-format changes
- No new enforcement behavior beyond M4.2
- No resource download/cache changes
- No execution of resources
- No silent fallback from Strict to ReportOnly
