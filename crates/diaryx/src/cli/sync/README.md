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

## Architecture

Sync commands are declared by the sync plugin's manifest (`CliCommand` entries
with `native_handler` fields) and dynamically added to the CLI at startup via
`plugin_dispatch.rs`. Each sync subcommand maps to a native handler function
in this module (e.g., `sync_login` → `auth::handle_login`).

Sync operations still go through the Extism sync plugin (`diaryx_sync.wasm`)
loaded at runtime via `CliSyncContext` (see `plugin_loader.rs`). The CLI is the
one remaining host exception during the boundary cleanup: some sync commands
still use native handlers, but transport now goes through the same generic
`TokioWebSocketBridge` host bridge used by the broader Extism host surface while
web/Tauri move toward fully plugin-owned command flows. Native Diaryx account
session state now lives under `diaryx_core::auth` (`auth.toml` beside
`config.toml`), and CLI sync client/status/runtime context read server/auth/
workspace inputs from that auth store rather than directly from `Config.sync_*`.

## Commands

All sync commands are plugin-declared and dispatched via `NativeHandlerRegistry`:

- `sync login` - Authenticate via magic link (`native_handler: sync_login`)
- `sync verify` - Complete authentication with token (`native_handler: sync_verify`)
- `sync logout` - Clear credentials (`native_handler: sync_logout`)
- `sync status` - Show sync status (`native_handler: sync_status`)
- `sync start` - Start continuous sync via the generic websocket host bridge (`native_handler: sync_start`)
- `sync push` - One-shot push local changes (`native_handler: sync_push`)
- `sync pull` - One-shot pull remote changes (`native_handler: sync_pull`)
- `sync config` - Configure sync settings (`native_handler: sync_config`)
