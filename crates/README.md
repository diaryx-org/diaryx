---
title: crates
description: Cargo crates for Diaryx
author: adammharris
contents:
  - "[Diaryx CLI README](/crates/diaryx/README.md)"
  - "[Diaryx Apple README](/crates/diaryx_apple/README.md)"
  - "[Diaryx Core README](/crates/diaryx_core/README.md)"
  - "[Diaryx Publish README](/crates/diaryx_publish/README.md)"
  - "[Diaryx Publish Extism README](/crates/diaryx_publish_extism/README.md)"
  - "[Diaryx WASM README](/crates/diaryx_wasm/README.md)"
  - "[Diaryx Sync Server README](/crates/diaryx_sync_server/README.md)"
  - "[Diaryx Sync Protocol README](/crates/diaryx_sync/README.md)"
  - "[S3 Storage Plugin README](/crates/diaryx_storage_s3_extism/README.md)"
  - "[Google Drive Storage Plugin README](/crates/diaryx_storage_gdrive_extism/README.md)"
exclude:
  - LICENSE.md
part_of: "[README](/README.md)"
---

# Diaryx Crates

This folder contains Diaryx crates for core features and platform bindings.

- `[diaryx](/crates/diaryx/README.md)`: CLI interface
- `[diaryx_apple](/crates/diaryx_apple/README.md)`: UniFFI bridge crate for Apple clients
- `[diaryx_core](/crates/diaryx_core/README.md)`: Core functions shared across all Diaryx clients
- `[diaryx_publish](/crates/diaryx_publish/README.md)`: Publishing pipeline — converts workspace markdown to HTML
- `[diaryx_publish_extism](/crates/diaryx_publish_extism/README.md)`: Extism guest plugin for export/publish commands and runtime format conversion
- `[diaryx_wasm](/crates/diaryx_wasm/README.md)`: WASM version of `diaryx_core` to be used in the web client at `[../apps/web](/apps/web/README.md)`
- `[diaryx_sync_server](/crates/diaryx_sync_server/README.md)`: Sync server to enable live sync/multi-device sync (soon publishing as well).
- `[diaryx_sync](/crates/diaryx_sync/README.md)`: Shared sync protocol library used by both `diaryx_sync_server` and the CLI's local web editing mode.
- `[diaryx_storage_s3_extism](/crates/diaryx_storage_s3_extism/README.md)`: S3-compatible storage as AsyncFileSystem Extism plugin
- `[diaryx_storage_gdrive_extism](/crates/diaryx_storage_gdrive_extism/README.md)`: Google Drive storage as AsyncFileSystem Extism plugin

Cargo also copies the LICENSE.md file here for publishing crates. It is has the same content as `../LICENSE.md`.
