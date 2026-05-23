# Server Browser

## Current Scope

Milestone 0.6 adds a standalone server browser skeleton. It is GTA-independent and uses a local JSON file source only.

## Current Source

- local file source: `examples/servers.local.json`
- loaded by `server_browser::LocalJsonServerListSource`
- printed by client with `--server-list`

Example:

```bash
cargo run -p client -- --server-list examples/servers.local.json
```

## Current Behavior

- parse local server metadata
- validate basic entry sanity
- filter by current protocol version
- print readable terminal list
- do not auto-connect

## Metadata Model

Each entry includes:

- name
- address
- port
- current players
- max players
- protocol version
- tags
- edition compatibility: `legacy`, `enhanced`, `any`, `unknown`

## Future Design

- signed master-server index
- filtering by edition, protocol, tags, and possibly ping
- optional local cache
- trust and provenance checks for published server indexes

## Privacy Considerations

- local JSON source avoids unsolicited network discovery
- no telemetry in this milestone
- no Rockstar services involved
- future hosted indexes should document retention, abuse controls, and operator visibility

## Clean-Room Note

Server discovery design must remain original. Do not copy proprietary discovery APIs, service schemas, or private launcher workflows from GTA V multiplayer ecosystems.
