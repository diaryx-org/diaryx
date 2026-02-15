---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
- '[Diaryx CLI README](/crates/diaryx/README.md)'
- '[Diaryx Core README](/crates/diaryx_core/README.md)'
- '[Diaryx WASM README](/crates/diaryx_wasm/README.md)'
- '[Diaryx Sync Server README](/crates/diaryx_sync_server/README.md)'
- '[Diaryx Sync Protocol README](/crates/diaryx_sync/README.md)'
exclude:
  - 'LICENSE.md'
part_of: '[README](/README.md)'
---

# Diaryx Crates

This folder contains four crates for Diaryx.

- [`diaryx`](/crates/diaryx/README.md): CLI interface
- [`diaryx_core`](/crates/diaryx_core/README.md): Core functions shared across all Diaryx clients
- [`diaryx_wasm`](/crates/diaryx_wasm/README.md): WASM version of `diaryx_core` to be used in the web client at [`../apps/web`](/apps/web/README.md)
- [`diaryx_sync_server`](/crates/diaryx_sync_server/README.md): Sync server to enable live sync/multi-device sync (soon publishing as well).
- [`diaryx_sync`](/crates/diaryx_sync/README.md): Shared sync protocol library used by both `diaryx_sync_server` and the CLI's local web editing mode.

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
