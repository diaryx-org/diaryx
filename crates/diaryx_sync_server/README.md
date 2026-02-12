---
title: diaryx_sync_server
description: Sync server used by frontends
author: adammharris
audience:
  - public
  - developers
part_of: "[README](/crates/README.md)"
contents:
  - "[README](/crates/diaryx_sync_server/src/README.md)"
attachments:
  - "[Cargo.toml](/crates/diaryx_sync_server/Cargo.toml)"
  - "[build.rs](/crates/diaryx_sync_server/build.rs)"
exclude:
  - "*.lock"
  - "*.db"
---

# Diaryx Sync Server

A Rust-based multi-device sync server for Diaryx with magic link authentication.

## Features

- **Magic link authentication**: Passwordless login via email
- **Real-time sync**: WebSocket-based Y-sync protocol using diaryx_core's CRDT infrastructure
- **Multi-device support**: Track and manage connected devices
- **Live share sessions**: Real-time collaboration with guests via shareable codes
- **Persistent storage**: SQLite-based storage for user data and CRDT state
- **Static site hosting pipeline**: Publish audience-filtered HTML to a dedicated R2 bucket

## Quick Start

```bash
# Set required environment variables
export RESEND_API_KEY=re_xxxx
export EMAIL_FROM=noreply@yourapp.com
export APP_BASE_URL=https://yourapp.com

# Run the server
cargo run -p diaryx_sync_server
```

## Environment Variables

| Variable                    | Default                                       | Description                                           |
| --------------------------- | --------------------------------------------- | ----------------------------------------------------- |
| `HOST`                      | `0.0.0.0`                                     | Server host                                           |
| `PORT`                      | `3030`                                        | Server port                                           |
| `DATABASE_PATH`             | `./diaryx_sync.db`                            | Path to SQLite database                               |
| `APP_BASE_URL`              | `http://localhost:5174`                       | Base URL for magic link verification                  |
| `RESEND_API_KEY`            | -                                             | Resend API key                                        |
| `EMAIL_FROM`                | `noreply@diaryx.org`                          | From email address                                    |
| `EMAIL_FROM_NAME`           | `Diaryx`                                      | From name                                             |
| `SESSION_EXPIRY_DAYS`       | `30`                                          | Session token expiration in days                      |
| `MAGIC_LINK_EXPIRY_MINUTES` | `15`                                          | Magic link expiration in minutes                      |
| `CORS_ORIGINS`              | `http://localhost:5174,http://localhost:5175` | Comma-separated CORS origins (methods: GET, POST, PUT, PATCH, DELETE, OPTIONS; headers: Authorization, Content-Type, Cache-Control, Pragma) |
| `SNAPSHOT_UPLOAD_MAX_BYTES` | `1073741824`                                  | Max snapshot upload size accepted by the API          |
| `R2_BUCKET`                 | `diaryx-user-data`                            | Cloudflare R2 bucket for attachment blobs             |
| `R2_ACCOUNT_ID`             | -                                             | Cloudflare account ID                                 |
| `R2_ACCESS_KEY_ID`          | -                                             | R2 access key ID                                      |
| `R2_SECRET_ACCESS_KEY`      | -                                             | R2 secret access key                                  |
| `R2_ENDPOINT`               | -                                             | Optional custom S3 endpoint override                  |
| `R2_PREFIX`                 | `diaryx-sync`                                 | Object key prefix inside the bucket                   |
| `R2_GC_RETENTION_DAYS`      | `7`                                           | Soft-delete retention before blob garbage collection  |
| `ATTACHMENT_INCREMENTAL_SYNC_ENABLED` | `true`                              | Enable incremental multipart attachment APIs          |
| `SITES_R2_BUCKET`           | `diaryx-sites`                                | Cloudflare R2 bucket for published static site files  |
| `PUBLISHED_SITE_LIMIT`      | `1`                                           | Per-user max published sites                          |
| `SITES_BASE_URL`            | `APP_BASE_URL`                                | Public base URL used when generating tokenized links  |
| `TOKEN_SIGNING_KEY`         | `AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=` | Base64 32-byte HMAC key shared with Worker            |

## API Endpoints

### Authentication

#### Request Magic Link

```
POST /auth/magic-link
Content-Type: application/json

{ "email": "user@example.com" }
```

Response:

```json
{
  "success": true,
  "message": "Check your email for a sign-in link."
}
```

#### Verify Magic Link

```
GET /auth/verify?token=XXX&device_name=My%20Device
```

Response:

```json
{
  "success": true,
  "token": "session_token_here",
  "user": {
    "id": "user_id",
    "email": "user@example.com"
  }
}
```

#### Get Current User

```
GET /auth/me
Authorization: Bearer <session_token>
```

#### Logout

```
POST /auth/logout
Authorization: Bearer <session_token>
```

#### List Devices

```
GET /auth/devices
Authorization: Bearer <session_token>
```

#### Delete Device

```
DELETE /auth/devices/{device_id}
Authorization: Bearer <session_token>
```

### API

#### Server Status

```
GET /api/status
```

Response:

```json
{
  "status": "ok",
  "version": "0.10.0",
  "active_connections": 5,
  "active_rooms": 2
}
```

#### List Workspaces

```
GET /api/workspaces
Authorization: Bearer <session_token>
```

#### Download Workspace Snapshot

```
GET /api/workspaces/{workspace_id}/snapshot?include_attachments=true|false
Authorization: Bearer <session_token>
```

Response: zip archive containing markdown files with frontmatter and (optionally)
attachment binaries resolved from blob storage.

#### Upload Workspace Snapshot

```
POST /api/workspaces/{workspace_id}/snapshot?mode=replace|merge&include_attachments=true|false
Authorization: Bearer <session_token>
Content-Type: application/zip
```

Response:

```json
{ "files_imported": 123 }
```

#### User Attachment Storage Usage

```
GET /api/user/storage
Authorization: Bearer <session_token>
```

Response:

```json
{
  "used_bytes": 123456,
  "blob_count": 42,
  "limit_bytes": 1073741824,
  "warning_threshold": 0.8,
  "over_limit": false,
  "scope": "attachments"
}
```

`limit_bytes` is per-user. New/existing users default to 1 GiB unless the
`users.attachment_limit_bytes` value is changed in the database.

#### Incremental Attachment Upload (Resumable)

```
POST /api/workspaces/{workspace_id}/attachments/uploads
Authorization: Bearer <session_token>
Content-Type: application/json
```

Initializes or resumes a multipart attachment upload session.
If an upload fits in a single part, the server uses a direct object upload path
internally (no remote multipart session is created).

```
PUT /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/parts/{part_no}
Authorization: Bearer <session_token>
Content-Type: application/octet-stream
```

Uploads one part for a multipart session.

```
POST /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/complete
Authorization: Bearer <session_token>
Content-Type: application/json
```

Completes upload and registers blob metadata for dedupe/usage accounting.

If the user's attachment usage would exceed their limit, upload initialization
or completion returns:

```json
{
  "error": "storage_limit_exceeded",
  "message": "Attachment storage limit exceeded",
  "used_bytes": 123,
  "limit_bytes": 1073741824,
  "requested_bytes": 456
}
```

#### Attachment Download by Hash

```
GET /api/workspaces/{workspace_id}/attachments/{hash}
Authorization: Bearer <session_token>
Range: bytes=start-end (optional)
```

Returns attachment bytes for hashes referenced by the workspace.

#### Published Site Management

```
POST   /api/workspaces/{workspace_id}/site
GET    /api/workspaces/{workspace_id}/site
DELETE /api/workspaces/{workspace_id}/site
POST   /api/workspaces/{workspace_id}/site/publish
POST   /api/workspaces/{workspace_id}/site/tokens
GET    /api/workspaces/{workspace_id}/site/tokens
DELETE /api/workspaces/{workspace_id}/site/tokens/{token_id}
Authorization: Bearer <session_token>
```

These endpoints manage static site configuration, trigger publish jobs, and
issue/revoke audience-scoped access tokens for the Cloudflare Worker.
Audience builds (including `public`) use frontmatter audience filtering; files
without explicit or inherited audience are excluded by default.
Each publish replaces prior artifacts under `/{slug}/{audience}/` to avoid
stale files remaining accessible.

### Share Sessions (Live Collaboration)

Share sessions allow real-time collaboration with guests who don't need accounts.

#### Create Session

```
POST /api/sessions
Authorization: Bearer <session_token>
Content-Type: application/json

{ "workspace_id": "uuid", "read_only": false }
```

Response:

```json
{
  "code": "XXXXXXXX-XXXXXXXX",
  "workspace_id": "uuid",
  "read_only": false
}
```

#### Get Session Info

```
GET /api/sessions/{code}
```

Response:

```json
{
  "code": "XXXXXXXX-XXXXXXXX",
  "workspace_id": "uuid",
  "read_only": false,
  "peer_count": 2
}
```

#### Update Session (toggle read-only)

```
PATCH /api/sessions/{code}
Authorization: Bearer <session_token>
Content-Type: application/json

{ "read_only": true }
```

#### End Session

```
DELETE /api/sessions/{code}
Authorization: Bearer <session_token>
```

### WebSocket Sync

The server supports two types of document sync:

1. **Workspace sync** - Syncs file metadata (title, part_of, contents, etc.)
2. **Body doc sync** - Syncs file body content (per-file documents)

This separation prevents large file bodies from bloating the workspace CRDT.

#### Workspace Sync (metadata only)

##### Authenticated (multi-device)

```
GET /sync?doc=workspace_id&token=session_token
```

##### Session Guest

```
GET /sync?session=XXXXXXXX-XXXXXXXX&guest_id=guest-123
```

#### Body Doc Sync (per-file body content)

##### Authenticated (multi-device)

```
GET /sync?doc=workspace_id&file=path/to/file.md&token=session_token
```

##### Session Guest

```
GET /sync?session=XXXXXXXX-XXXXXXXX&file=path/to/file.md&guest_id=guest-123
```

The WebSocket connection uses the Y-sync protocol (compatible with y-protocols). Binary messages are Y.js updates, text messages are control messages (peer_joined, peer_left, read_only_changed, session_ended).

## Architecture

```
┌─────────────────┐                    ┌─────────────────────────┐
│   Web/Tauri     │◄──── WS (metadata) │  diaryx_sync_server     │
│   Client        │◄──── WS (body 1)   │  (Rust + axum)          │
│                 │◄──── WS (body 2)   │                         │
└─────────────────┘◄──── WS (body N)   │  ┌─────────────────┐    │
                                       │  │  SyncRoom       │    │
                                       │  │  - WorkspaceCrdt│ metadata
                                       │  │  - BodyDocMgr   │ per-file
                                       │  │  - SqliteStorage│    │
                                       │  └─────────────────┘    │
                                       │           │             │
                                       │           ▼             │
                                       │  ┌─────────────────┐    │
                                       │  │    SQLite DB    │    │
                                       │  └─────────────────┘    │
                                       └─────────────────────────┘
```

Each file being edited has its own WebSocket connection for body content sync.
Workspace metadata (titles, hierarchy) syncs via a single shared connection.

## Development

### Running Locally

```bash
# Without email (dev mode - magic link returned in response)
cargo run -p diaryx_sync_server

# With email
SMTP_HOST=smtp.mailtrap.io \
SMTP_USERNAME=xxx \
SMTP_PASSWORD=xxx \
SMTP_FROM_EMAIL=test@example.com \
cargo run -p diaryx_sync_server
```

### Testing

```bash
# Run all tests (unit + E2E)
cargo test -p diaryx_sync_server

# Run only E2E integration tests
cargo test -p diaryx_sync_server --test e2e_sync -- --nocapture
```

#### E2E Test Suite

The `tests/e2e_sync.rs` file contains comprehensive end-to-end integration tests that verify the full sync pipeline with real WebSocket connections to an in-memory test server:

- **test_basic_push_pull**: Client A pushes files, Client B pulls and verifies identical state
- **test_bidirectional_sync**: Two clients sync their unique files to share all data
- **test_incremental_update**: Verifies incremental updates propagate correctly
- **test_large_workspace**: Tests sync with 100+ files for scalability
- **test_metadata_consistency**: Verifies complex metadata (contents, part_of, audience) survives sync
- **test_body_content_integrity**: Tests unicode, special characters, and large (>100KB) files
- **test_empty_update_detection**: Ensures no unnecessary updates when state is identical
- **test_concurrent_modifications**: Verifies CRDT merging with concurrent changes
