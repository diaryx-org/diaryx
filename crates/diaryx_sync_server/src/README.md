---
title: diaryx_sync_server src
description: Source code for the sync server
part_of: '[README](/crates/diaryx_sync_server/README.md)'
contents:
  - '[README](/crates/diaryx_sync_server/src/auth/README.md)'
  - '[README](/crates/diaryx_sync_server/src/db/README.md)'
  - '[README](/crates/diaryx_sync_server/src/email/README.md)'
  - '[README](/crates/diaryx_sync_server/src/handlers/README.md)'
  - '[README](/crates/diaryx_sync_server/src/sync/README.md)'
attachments:
  - '[lib.rs](/crates/diaryx_sync_server/src/lib.rs)'
  - '[main.rs](/crates/diaryx_sync_server/src/main.rs)'
  - '[config.rs](/crates/diaryx_sync_server/src/config.rs)'
  - '[git_ops.rs](/crates/diaryx_sync_server/src/git_ops.rs)'
exclude:
  - '*.lock'
---

# diaryx_sync_server Source

This directory contains the source code for the Diaryx sync server.

## Structure

| File | Purpose |
|------|---------|
| `lib.rs` | Library entry point |
| `main.rs` | Server entry point |
| `config.rs` | Configuration from environment variables |
| `blob_store.rs` | Attachment blob storage abstraction (R2/in-memory) |

## Modules

- `auth/` - Authentication middleware and magic link handling
- `blob_store.rs` - R2-backed attachment blob storage abstraction
- `db/` - SQLite database schema and repository
- `email/` - SMTP email sending
- `git_ops.rs` - Git operations (commit, restore) for server-side workspaces
- `handlers/` - HTTP route handlers
- `sync_v2/` - Siphonophore-based sync implementation
