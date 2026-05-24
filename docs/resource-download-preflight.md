Resource Download Preflight (M6.1)

Purpose:

- Expose deterministic, report-only resource download preflight planner via CLI.
- No network I/O, no cache writes, no execution, no mutation.

Usage:

- Offline announce file inspection:

  --plan-resource-downloads <announcement.json> [--resource-cache <path>] [--trusted-keys <path>] [--signature-policy strict|report_only]

- Examples:

  Inspect announcement with default report-only signature policy:

  client --plan-resource-downloads /path/to/announcement.json

  Inspect announcement and consider an existing local cache for availability:

  client --plan-resource-downloads /path/to/announcement.json --resource-cache /path/to/cache

Notes:

- Output is deterministic text from ResourceDownloadPreflightPlan::to_text().
- Command never performs downloads or cache writes. It only reads the announcement file and, optionally, inspects a local cache directory when --resource-cache is provided.
- When signature policy is strict, --trusted-keys is required and validated.

Source Metadata Reporting (M6.5):

- Each preflight entry includes source metadata from the announced resource file.
- Valid sources appear under `sources: N validated` in text output or `valid_sources` in JSON.
- Invalid sources (unsupported scheme, path traversal, SHA/size mismatch, duplicate) produce `source error:` lines in text output and `source_errors` entries in JSON.
- Source validation is deterministic: valid sources are sorted by URL for consistent ordering.
- Sources are validated per preflight entry, including synthetic `WouldVerifyAfterFetch` entries.
- All existing output and behavior is unchanged when a resource file has no `sources` field.
- Report-only: no source URL is ever used for fetching, network access, cache writes, or execution.

Source Selection Planning (M6.6):

- Each preflight entry identifies a `selected_source` (best candidate) and `fallback_sources` (remaining valid sources) from the validated source list.
- Selection is deterministic: lowest `priority` value wins; tie-break by `id` ascending, then `uri` ascending.
- When validation errors make all sources invalid, `selected_source` is `None` and `fallback_sources` is empty.
- Text output shows `selected source: <scheme> <uri>` and `fallback sources: N` per entry.
- JSON output includes `selected_source` and `fallback_sources` per entry; optional fields are omitted when empty (backward-compatible).
- Selections are attached per preflight entry, including synthetic `WouldVerifyAfterFetch` entries.
- Report-only: no source URL is ever used for fetching, network access, cache writes, or execution.
