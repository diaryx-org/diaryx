---
title: Database module
description: SQLite database schema and repository
part_of: '[README](/crates/diaryx_sync_server/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_sync_server/src/db/mod.rs)'
  - '[repo.rs](/crates/diaryx_sync_server/src/db/repo.rs)'
  - '[schema.rs](/crates/diaryx_sync_server/src/db/schema.rs)'
exclude:
  - '*.lock'
---

# Database Module

SQLite database layer for the sync server.

## Files

- `mod.rs` - Module exports and database initialization
- `repo.rs` - Repository pattern for database operations
- `schema.rs` - SQLite table schemas and migrations

The schema includes attachment usage tracking tables:

- `user_attachment_blobs` (per-user deduplicated blob metadata + ref counts)
- `workspace_attachment_refs` (workspace path refs to blob hashes)
- `attachment_uploads` (resumable multipart upload sessions)
- `attachment_upload_parts` (uploaded part ETags/state per upload session)
