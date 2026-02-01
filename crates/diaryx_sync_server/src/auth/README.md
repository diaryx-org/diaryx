---
title: Auth module
description: Authentication middleware and magic link handling
part_of: '[README](/crates/diaryx_sync_server/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_sync_server/src/auth/mod.rs)'
  - '[magic_link.rs](/crates/diaryx_sync_server/src/auth/magic_link.rs)'
  - '[middleware.rs](/crates/diaryx_sync_server/src/auth/middleware.rs)'
exclude:
  - '*.lock'
---

# Auth Module

Authentication handling for the sync server.

## Files

- `mod.rs` - Module exports
- `magic_link.rs` - Magic link token generation and verification
- `middleware.rs` - Axum middleware for session token authentication
