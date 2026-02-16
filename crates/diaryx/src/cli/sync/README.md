---
title: Sync CLI module
description: CLI commands for workspace synchronization
part_of: '[README](/crates/diaryx/src/cli/README.md)'
attachments:
- '[mod.rs](/crates/diaryx/src/cli/sync/mod.rs)'
- '[auth.rs](/crates/diaryx/src/cli/sync/auth.rs)'
- '[client.rs](/crates/diaryx/src/cli/sync/client.rs)'
- '[status.rs](/crates/diaryx/src/cli/sync/status.rs)'
- '[progress.rs](/crates/diaryx/src/cli/sync/progress.rs)'
exclude:
- '*.lock'
---
# Sync CLI Module

CLI commands for workspace synchronization with the Diaryx sync server.

## Commands

- `sync login` - Authenticate via magic link
- `sync verify` - Complete authentication with token
- `sync logout` - Clear credentials
- `sync status` - Show sync status
- `sync start` - Start continuous sync
- `sync push` - One-shot push local changes
- `sync pull` - One-shot pull remote changes

## Metadata Canonicalization

When the CLI imports or updates local files in CRDT state, frontmatter
references are canonicalized with `link_parser` (`part_of`, `contents`,
`attachments`). This keeps CRDT metadata path storage consistent and avoids
relative-path drift in nested workspaces.
