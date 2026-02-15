---
title: diaryx_wasm src
description: Source code for WASM bindings
part_of: '[README](/crates/diaryx_wasm/README.md)'
attachments:
  - '[lib.rs](/crates/diaryx_wasm/src/lib.rs)'
  - '[backend.rs](/crates/diaryx_wasm/src/backend.rs)'
  - '[error.rs](/crates/diaryx_wasm/src/error.rs)'
  - '[fsa_fs.rs](/crates/diaryx_wasm/src/fsa_fs.rs)'
  - '[indexeddb_fs.rs](/crates/diaryx_wasm/src/indexeddb_fs.rs)'
  - '[js_async_fs.rs](/crates/diaryx_wasm/src/js_async_fs.rs)'
  - '[opfs_fs.rs](/crates/diaryx_wasm/src/opfs_fs.rs)'
  - '[utils.rs](/crates/diaryx_wasm/src/utils.rs)'
  - '[wasm_sqlite_storage.rs](/crates/diaryx_wasm/src/wasm_sqlite_storage.rs)'
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
| `wasm_sqlite_storage.rs` | SQLite storage for WASM CRDT |

`backend.rs` keeps `setCrdtEnabled` synchronized between the control-side
filesystem handle and the command-execution `Diaryx` filesystem handle so guest
session edits correctly emit CRDT updates after sync bootstrap.
