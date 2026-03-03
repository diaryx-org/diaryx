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
- '[ws_bridge.rs](/crates/diaryx/src/cli/sync/ws_bridge.rs)'
exclude:
- '*.lock'
---
# Sync CLI Module

CLI commands for workspace synchronization with the Diaryx sync server.

## Architecture

Sync commands are declared by the sync plugin's manifest (`CliCommand` entries
with `native_handler` fields) and dynamically added to the CLI at startup via
`plugin_dispatch.rs`. Each sync subcommand maps to a native handler function
in this module (e.g., `sync_login` → `auth::handle_login`).

Sync operations go through the Extism sync plugin (`diaryx_sync.wasm`) loaded
at runtime via `CliSyncContext` (see `plugin_loader.rs`). The WebSocket
transport is handled by `WsBridge` (`ws_bridge.rs`), which bridges
`tokio-tungstenite` frames to the plugin's binary action protocol.

## Commands

All sync commands are plugin-declared and dispatched via `NativeHandlerRegistry`:

- `sync login` - Authenticate via magic link (`native_handler: sync_login`)
- `sync verify` - Complete authentication with token (`native_handler: sync_verify`)
- `sync logout` - Clear credentials (`native_handler: sync_logout`)
- `sync status` - Show sync status (`native_handler: sync_status`)
- `sync start` - Start continuous sync via WsBridge (`native_handler: sync_start`)
- `sync push` - One-shot push local changes (`native_handler: sync_push`)
- `sync pull` - One-shot pull remote changes (`native_handler: sync_pull`)
- `sync config` - Configure sync settings (`native_handler: sync_config`)
