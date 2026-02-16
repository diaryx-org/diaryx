---
title: Handlers module
description: HTTP route handlers
part_of: "[README](/crates/diaryx_sync_server/src/README.md)"
attachments:
  - "[mod.rs](/crates/diaryx_sync_server/src/handlers/mod.rs)"
  - "[api.rs](/crates/diaryx_sync_server/src/handlers/api.rs)"
  - "[auth.rs](/crates/diaryx_sync_server/src/handlers/auth.rs)"
  - "[sites.rs](/crates/diaryx_sync_server/src/handlers/sites.rs)"
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
| `api.rs`      | General API endpoints (status, workspace CRUD)        |
| `auth.rs`     | Authentication endpoints (magic-link, verify, logout) |
| `sites.rs`    | Published site and access-token management endpoints   |
| `sessions.rs` | Share session management endpoints                    |
| `ws.rs`       | WebSocket upgrade and sync handling                   |

### Workspace CRUD Endpoints

- `GET /api/workspaces` — list user's workspaces
- `POST /api/workspaces` — create workspace (body: `{"name": "..."}`, enforces per-user workspace limit)
- `GET /api/workspaces/{id}` — get workspace info
- `PATCH /api/workspaces/{id}` — rename workspace (body: `{"name": "..."}`)
- `DELETE /api/workspaces/{id}` — delete workspace + cleanup (git repo, CRDT storage, attachment refs)

Workspace creation returns `403` when the user's workspace limit is reached, and `409` for duplicate names.
The per-user workspace limit defaults to the user's tier (Free=1, Plus=10) and can be overridden via `workspace_limit` on the users table.
The `GET /auth/me` response includes `workspace_limit`, `tier`, `published_site_limit`, and `attachment_limit_bytes`.

### Snapshot Endpoints

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
- Completing an upload now triggers immediate workspace attachment-ref
  reconciliation, so newly completed blobs become downloadable without waiting
  for another workspace metadata edit.

For one-part uploads, the handler uses a direct blob `put` path internally and
skips remote multipart completion/abort calls.
- `GET /api/workspaces/{workspace_id}/attachments/{hash}` — download attachment bytes (supports `Range`).

Attachment uploads and attachment-enabled snapshot imports enforce per-user
attachment limits. Over-limit requests return `413` with
`error: "storage_limit_exceeded"` JSON.

### Git Version History Endpoints

- `GET /api/workspaces/{id}/history?count=N` — Commit log from bare repo
- `POST /api/workspaces/{id}/commit` — Trigger immediate git commit (body: `{"message": "..."}`)
- `POST /api/workspaces/{id}/restore` — Rebuild CRDT from target commit (body: `{"commit_id": "..."}`, requires peer_count <= 1)

### Published Site Endpoints

- `POST /api/workspaces/{id}/site` — create published site config (`slug`, optional `enabled`, optional `auto_publish`).
- `GET /api/workspaces/{id}/site` — fetch site config + per-audience build status.
- `DELETE /api/workspaces/{id}/site` — unpublish site and delete static artifacts from the sites bucket.
- `POST /api/workspaces/{id}/site/publish` — trigger immediate publish to the sites bucket.
- `POST /api/workspaces/{id}/site/tokens` — create signed access token (`audience`, optional `label`, optional `expires_in`).
- `GET /api/workspaces/{id}/site/tokens` — list token metadata for the workspace site.
- `DELETE /api/workspaces/{id}/site/tokens/{token_id}` — revoke a token and refresh `_meta.json` revocation list.

### Admin Endpoints

- `PUT /api/admin/users/{user_id}/tier` — set a user's tier (body: `{"tier": "free"|"plus"}`). Requires `X-Admin-Secret` header matching the `ADMIN_SECRET` env var. Returns `204` on success, `401` on bad secret, `404` if no admin secret configured or user not found, `400` on invalid tier.
