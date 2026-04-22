---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
- '[README](/crates/diaryx/README.md)'
- '[README](/crates/diaryx_server/README.md)'
- '[README](/crates/diaryx_core/README.md)'
- '[README](/crates/diaryx_wasm/README.md)'
- '[README](/crates/diaryx_sync_server/README.md)'
- '[README](/crates/diaryx_extism/README.md)'
- '[README](/crates/plugins/diaryx_plugin_sdk/README.md)'
- '[README](/crates/plugins/diaryx_sync_extism/README.md)'
- '[README](/crates/plugins/diaryx_publish_extism/README.md)'
exclude:
- LICENSE.md
- '**/*.rs'
part_of: '[Diaryx](/Diaryx.md)'
audience:
- developers
- agents
---

# Diaryx Crates

This folder contains Diaryx crates for core features and platform bindings.

- `[diaryx](/crates/diaryx/README.md)`: CLI interface
- `[diaryx_server](/crates/diaryx_server/README.md)`: Platform-agnostic server core and adapter contracts
- `[diaryx_core](/crates/diaryx_core/README.md)`: Core functions shared across all Diaryx clients
- `[diaryx_wasm](/crates/diaryx_wasm/README.md)`: WASM version of `diaryx_core` to be used in the web client at `[../apps/web](/apps/web/README.md)`
- `[diaryx_sync_server](/crates/diaryx_sync_server/README.md)`: Sync server to enable live sync/multi-device sync (soon publishing as well). The platform-agnostic CRDT/sync primitives it and `diaryx_cloudflare` share now live in `diaryx_server::sync`.
- `[diaryx_extism](/crates/diaryx_extism/README.md)`: Extism host runtime — loads WASM guest plugins at runtime
- `tauri-plugin-icloud`: Tauri plugin for iCloud Drive workspace storage (iOS)
- `tauri-plugin-iap`: Tauri plugin for Apple In-App Purchases via StoreKit 2
- `tauri-plugin-editor-toolbar`: native iOS keyboard toolbar for the shared TipTap editor, including block/inline formatting, links, plugin commands, and audience visibility selection

### Plugin crates (`plugins/`)

- `[diaryx_plugin_sdk](/crates/plugins/diaryx_plugin_sdk/README.md)`: SDK for building Diaryx Extism WASM plugins (published to crates.io)
- `[diaryx_sync_extism](/crates/plugins/diaryx_sync_extism/README.md)`: Sync plugin — real-time multi-device sync (Extism guest WASM)
- `[diaryx_publish_extism](/crates/plugins/diaryx_publish_extism/README.md)`: Publish plugin — export and website publishing (Extism guest WASM)

Other plugin guest crates (daily, AI, math, spoiler, import, templating, S3 storage, Google Drive storage, and friends) now live in `crates/plugins/` in this monorepo.

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
