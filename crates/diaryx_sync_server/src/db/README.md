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

- `users.tier` (user tier: `free` or `plus`; controls default limits)
- `users.attachment_limit_bytes` (per-user attachment quota override; NULL falls back to tier default)
- `users.workspace_limit` (per-user workspace count limit override; NULL falls back to tier default)
- `users.published_site_limit` (per-user published site limit override; NULL falls back to tier default)
- `user_attachment_blobs` (per-user deduplicated blob metadata + ref counts)
- `workspace_attachment_refs` (workspace path refs to blob hashes)
- `attachment_uploads` (resumable multipart upload sessions)
- `attachment_upload_parts` (uploaded part ETags/state per upload session)
- `published_sites` (workspace-to-site slug mapping and publish settings)
- `site_audience_builds` (latest build file counts per audience)
- `site_access_tokens` (audience-scoped token metadata + revocation state)

`attachment_uploads` is also queried during sync-v2 reconciliation as a fallback
source of hash/size/mime metadata when a workspace attachment ref is present but
its synced `BinaryRef.hash` is empty.

`repo.rs` now hard-reconciles `user_attachment_blobs.ref_count` from
`workspace_attachment_refs` after workspace attachment-ref replacement, which
prevents long-lived storage-usage drift after large imports, sync mode changes,
or stale ref-count updates.
