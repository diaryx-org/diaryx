---
title: Filesystem module
description: Filesystem abstraction layer
part_of: '[README](/crates/diaryx_core/src/README.md)'
exclude:
  - '*.lock'
  - '**/*.rs'
---

# Filesystem Module

This module provides filesystem abstraction through `FileSystem` (synchronous) and `AsyncFileSystem` (asynchronous) traits.

## Implementations

| File | Purpose |
|------|---------|
| `async_fs.rs` | Async filesystem trait and `SyncToAsyncFs` adapter |
| `memory.rs` | In-memory filesystem (portable; used by tests, WASM, and web client) |
| `event_fs.rs` | Event-emitting filesystem decorator |
| `events.rs` | Filesystem event types |
| `callback_registry.rs` | Callback management for events |
| `decorator_stack.rs` | Composable filesystem decorators |

Platform-specific backends live in sibling crates:

| Crate            | Backend                                                    |
|------------------|------------------------------------------------------------|
| `diaryx_native`  | `RealFileSystem` — `std::fs` (CLI/Tauri desktop + mobile)  |
| `diaryx_wasm`    | OPFS / IndexedDB / File System Access (browser)            |

## Backend Parity

`InMemoryFileSystem` and `diaryx_native::RealFileSystem` now both auto-create missing parent
directories for `write_file`, `write_binary`, and `create_new` operations. This
keeps behavior consistent across web/native backends for nested path writes,
including attachment/media uploads.

## Safe-write Recovery

Metadata/frontmatter writes use a temp + backup swap strategy. On OPFS/FSA,
there can be transient races where `exists(path)` is true but moving
`path -> path.bak` returns a `NotFound`/missing-object error.

`metadata_writer` now treats that backup-move race as recoverable and continues
the swap, with a direct-overwrite fallback for transient `NotFound`,
`AlreadyExists`, and browser permission/state write errors. This prevents edit
operations from getting stuck in retry loops during sync churn.
