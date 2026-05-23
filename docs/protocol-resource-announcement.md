# Protocol Resource Announcement

## Purpose

Milestone 1.3 connects the protocol layer to the resource system with a safe resource announcement flow. The server announces required resources, and the client reports whether local files are available and valid.

## Current Scope

- local only
- no downloads
- no repair
- no execution
- no file contents in protocol
- deterministic ordering

## Announcement Flow

- server sends `ServerMessage::ResourceAnnouncement`
- client prints announced resources and files
- client checks local cache state if a cache path is configured
- client sends `ClientMessage::ResourceAvailabilityReport`

## What Client Verifies

- file presence
- file size
- file hash

This reuses existing cache verification behavior.

## What Protocol Does Not Send

- file contents
- scripts
- binaries to execute
- resource payloads

## Current Limitations

- no signed announcements yet
- no resource download or repair yet
- exact protocol version match remains in effect

## Future Work

- signed announcements
- resource download pipeline
- repair/update workflow
- stronger per-resource compatibility metadata

## Clean-Room Note

Resource announcement and availability reporting must remain original. Do not copy proprietary distribution manifests, patch protocols, or private launcher/resource sync behavior from GTA V multiplayer ecosystems.

## Edition Independence

This announcement flow is independent from GTA V Legacy and Enhanced because it only exchanges metadata about local resource presence and integrity.
