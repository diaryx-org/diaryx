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
Snapshot endpoints support `include_attachments=true|false` query params.
Snapshot uploads also support `mode=replace|merge` and enforce a configurable
max payload size (`SNAPSHOT_UPLOAD_MAX_BYTES`, default 1 GiB).

`api.rs` also serves per-user attachment usage at:

- `GET /api/user/storage` — returns used bytes/blob count for synced attachment blobs.
- `POST /api/workspaces/{workspace_id}/attachments/uploads` — initialize/resume multipart attachment upload.
- `PUT /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/parts/{part_no}` — upload one part.
- `POST /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/complete` — finalize multipart upload.
- `GET /api/workspaces/{workspace_id}/attachments/{hash}` — download attachment bytes (supports `Range`).

### Git Version History Endpoints

- `GET /api/workspaces/{id}/history?count=N` — Commit log from bare repo
- `POST /api/workspaces/{id}/commit` — Trigger immediate git commit (body: `{"message": "..."}`)
- `POST /api/workspaces/{id}/restore` — Rebuild CRDT from target commit (body: `{"commit_id": "..."}`, requires peer_count <= 1)
