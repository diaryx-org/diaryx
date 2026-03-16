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
- `passkey.rs` - WebAuthn/passkey registration and authentication

## Device limit & replacement

When a user hits the device limit during sign-in (magic link, verification code, or passkey), all three verification methods accept an optional `replace_device_id` parameter. If supplied, the old device is deleted and the new one is registered atomically. The `DeviceLimitReached` error includes the list of existing devices so the client can prompt the user to choose which device to replace.
