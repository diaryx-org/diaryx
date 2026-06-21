---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
- '[README](/crates/crossfs/README.md)'
- '[README](/crates/diaryx/README.md)'
- '[README](/crates/diaryx_server/README.md)'
- '[README](/crates/diaryx_core/README.md)'
- '[README](/crates/diaryx_wasm/README.md)'
- '[README](/crates/diaryx_selfhosted/README.md)'
- '[README](/crates/diaryx_extism/README.md)'
- '[README](/crates/plugins/diaryx_plugin_sdk/README.md)'
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

- `[bookmatter](/crates/bookmatter/README.md)`: Order-preserving, round-trip metadata parser for plain text files
- `[crossfs](/crates/crossfs/README.md)`: Cross-platform filesystem abstraction for Rust mirroring `std::fs` / `tokio::fs`
- `[diaryx](/crates/diaryx/README.md)`: CLI interface
- `[diaryx_server](/crates/diaryx_server/README.md)`: Platform-agnostic server core and adapter contracts
- `[diaryx_core](/crates/diaryx_core/README.md)`: Core functions shared across all Diaryx clients
- `[diaryx_wasm](/crates/diaryx_wasm/README.md)`: WASM version of `diaryx_core` to be used in the web client at `[../apps/web](/apps/web/README.md)`
- `[diaryx_selfhosted](/crates/diaryx_selfhosted/README.md)`: Self-hostable Diaryx backend (Axum + SQLite). A deployment target that, like `diaryx_cloudflare`, shares its server logic via `diaryx_server`.
- `[diaryx_extism](/crates/diaryx_extism/README.md)`: Extism host runtime — loads WASM guest plugins at runtime
- `tauri-plugin-icloud`: Tauri plugin for iCloud Drive workspace storage (iOS)
- `tauri-plugin-iap`: Tauri plugin for Apple In-App Purchases via StoreKit 2
- `tauri-plugin-editor-toolbar`: native iOS keyboard toolbar for the shared TipTap editor, including block/inline formatting, links, plugin commands, and audience visibility selection

### Plugin crates (`plugins/`)

- `[diaryx_plugin_sdk](/crates/plugins/diaryx_plugin_sdk/README.md)`: SDK for building Diaryx Extism WASM plugins (published to crates.io)

Other plugin guest crates (daily, AI, math, spoiler, import, templating, S3 storage, Google Drive storage, and friends) now live in `crates/plugins/` in this monorepo.

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
