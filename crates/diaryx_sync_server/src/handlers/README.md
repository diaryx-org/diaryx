---
title: Handlers module
description: HTTP route handlers
part_of: "[README](/crates/diaryx_sync_server/src/README.md)"
attachments:
  - "[mod.rs](/crates/diaryx_sync_server/src/handlers/mod.rs)"
  - "[api.rs](/crates/diaryx_sync_server/src/handlers/api.rs)"
  - "[auth.rs](/crates/diaryx_sync_server/src/handlers/auth.rs)"
  - "[sessions.rs](/crates/diaryx_sync_server/src/handlers/sessions.rs)"
  - "[ws.rs](/crates/diaryx_sync_server/src/handlers/ws.rs)"
exclude:
  - "*.lock"
---

# Handlers Module

HTTP route handlers for the sync server API.

## Files

| File          | Purpose                                               |
| ------------- | ----------------------------------------------------- |
| `mod.rs`      | Router setup and middleware                           |
| `api.rs`      | General API endpoints (status, workspaces)            |
| `auth.rs`     | Authentication endpoints (magic-link, verify, logout) |
| `sessions.rs` | Share session management endpoints                    |
| `ws.rs`       | WebSocket upgrade and sync handling                   |

`api.rs` also serves workspace snapshot downloads and uploads at
`GET /api/workspaces/{workspace_id}/snapshot` and
`POST /api/workspaces/{workspace_id}/snapshot`.
