---
title: Filesystem module
description: Filesystem abstraction layer
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/fs/mod.rs)'
  - '[async_fs.rs](/crates/diaryx_core/src/fs/async_fs.rs)'
  - '[callback_registry.rs](/crates/diaryx_core/src/fs/callback_registry.rs)'
  - '[crdt_fs.rs](/crates/diaryx_core/src/fs/crdt_fs.rs)'
  - '[decorator_stack.rs](/crates/diaryx_core/src/fs/decorator_stack.rs)'
  - '[event_fs.rs](/crates/diaryx_core/src/fs/event_fs.rs)'
  - '[events.rs](/crates/diaryx_core/src/fs/events.rs)'
  - '[memory.rs](/crates/diaryx_core/src/fs/memory.rs)'
  - '[native.rs](/crates/diaryx_core/src/fs/native.rs)'
exclude:
  - '*.lock'
---

# Filesystem Module

This module provides filesystem abstraction through `FileSystem` (synchronous) and `AsyncFileSystem` (asynchronous) traits.

## Implementations

| File | Purpose |
|------|---------|
| `async_fs.rs` | Async filesystem trait and `SyncToAsyncFs` adapter |
| `memory.rs` | In-memory filesystem (used by WASM/web client) |
| `native.rs` | Native filesystem using `std::fs` (CLI/Tauri) |
| `crdt_fs.rs` | CRDT-aware filesystem decorator |
| `event_fs.rs` | Event-emitting filesystem decorator |
| `events.rs` | Filesystem event types |
| `callback_registry.rs` | Callback management for events |
| `decorator_stack.rs` | Composable filesystem decorators |

## Sync Write Markers

`AsyncFileSystem::mark_sync_write_start/end` markers are forwarded through the
decorator stack (including `EventEmittingFs`) to `CrdtFs`. This prevents remote
sync writes from being re-emitted as new local CRDT updates.

`CrdtFs` also normalizes sync path keys before tracking local-write suppression,
so path aliases (`README.md`, `./README.md`, `/README.md`) map to one logical
file during sync echo handling.

Sync write suppression now applies to `move_file` and `delete_file` too (not
just `write_file`). This is critical for safe-write swaps (`file -> file.bak ->
file`) used by metadata updates during remote sync. Temp-file paths (`.tmp`,
`.bak`, `.swap`) are skipped for CRDT mutations, preventing transient swap
operations from corrupting workspace path state.

## Safe-write Recovery

Metadata/frontmatter writes use a temp + backup swap strategy. On OPFS/FSA,
there can be transient races where `exists(path)` is true but moving
`path -> path.bak` returns a `NotFound`/missing-object error.

`metadata_writer` now treats that backup-move race as recoverable and continues
the swap, with a direct-overwrite fallback for transient `NotFound`,
`AlreadyExists`, and browser permission/state write errors. This prevents edit
operations from getting stuck in retry loops during sync churn.
