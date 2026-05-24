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
