---
title: Sync module
description: WebSocket sync room management
part_of: "[README](/crates/diaryx_sync_server/src/README.md)"
attachments:
  - "[mod.rs](/crates/diaryx_sync_server/src/sync/mod.rs)"
  - "[connection.rs](/crates/diaryx_sync_server/src/sync/connection.rs)"
  - "[room.rs](/crates/diaryx_sync_server/src/sync/room.rs)"
exclude:
  - "*.lock"
---

# Sync Module

WebSocket sync room management using Y-sync protocol.

## Files

| File            | Purpose                                         |
| --------------- | ----------------------------------------------- |
| `mod.rs`        | Module exports and shared types                 |
| `connection.rs` | Individual WebSocket connection handling        |
| `room.rs`       | Sync room with WorkspaceCrdt and BodyDocManager |

`room.rs` also provides snapshot export/import helpers used by
`/api/workspaces/{workspace_id}/snapshot` for bulk bootstrap.
