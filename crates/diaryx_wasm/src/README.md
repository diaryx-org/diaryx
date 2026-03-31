---
title: diaryx_wasm src
description: Source code for WASM bindings
part_of: '[README](/crates/diaryx_wasm/README.md)'
exclude:
  - '*.lock'
---

# diaryx_wasm Source

This directory contains the source code for the WASM bindings.

## Files

| File | Purpose |
|------|---------|
| `lib.rs` | WASM entry point and exported classes |
| `backend.rs` | Backend abstraction for WASM |
| `error.rs` | JavaScript-compatible error handling |
| `fsa_fs.rs` | File System Access API filesystem |
| `indexeddb_fs.rs` | IndexedDB-based filesystem |
| `js_async_fs.rs` | JavaScript async filesystem adapter |
| `opfs_fs.rs` | Origin Private File System filesystem |
| `utils.rs` | Utility functions for WASM |

## Architecture

`diaryx_wasm` depends only on `diaryx_core`. Sync and publish functionality
are provided by Extism guest plugins (`diaryx_sync.wasm`, `diaryx_publish.wasm`)
loaded at runtime by the browser plugin manager.

`backend.rs` exposes import parsing functions (`parseDayOneJson`, `parseMarkdownFile`)
that take raw file bytes from JavaScript and return parsed entries as JSON. These
are consumed by the web app's import flows and plugin host actions. The actual entry
writing is handled by the `ImportEntries` command through `execute()`/`executeJs()`.
