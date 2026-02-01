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
