---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
- '[README](/crates/diaryx/README.md)'
- '[README](/crates/diaryx_server/README.md)'
- '[README](/crates/diaryx_core/README.md)'
- '[README](/crates/diaryx_daily/README.md)'
- '[README](/crates/diaryx_wasm/README.md)'
- '[README](/crates/diaryx_sync_server/README.md)'
- '[README](/crates/diaryx_sync/README.md)'
- '[README](/crates/diaryx_extism/README.md)'
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
- `[diaryx_daily](/crates/diaryx_daily/README.md)`: Shared daily-entry domain logic for plugins
- `[diaryx_wasm](/crates/diaryx_wasm/README.md)`: WASM version of `diaryx_core` to be used in the web client at `[../apps/web](/apps/web/README.md)`
- `[diaryx_sync_server](/crates/diaryx_sync_server/README.md)`: Sync server to enable live sync/multi-device sync (soon publishing as well).
- `[diaryx_sync](/crates/diaryx_sync/README.md)`: Shared sync protocol library used by both `diaryx_sync_server` and the CLI's local web editing mode.
- `[diaryx_extism](/crates/diaryx_extism/README.md)`: Extism host runtime — loads WASM guest plugins at runtime
- `tauri-plugin-icloud`: Tauri plugin for iCloud Drive workspace storage (iOS)
- `tauri-plugin-iap`: Tauri plugin for Apple In-App Purchases via StoreKit 2

Plugin guest crates (sync, publish, daily, AI, math, spoiler, import, templating, S3 storage, Google Drive storage) now live in standalone repos under [diaryx-org](https://github.com/diaryx-org).

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
