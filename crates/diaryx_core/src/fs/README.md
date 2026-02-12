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

## Frontmatter Path Canonicalization

`CrdtFs` canonicalizes frontmatter references before writing metadata to CRDT:

- `part_of`
- `contents[]`
- `attachments[]`

It resolves links through `link_parser` (supports markdown links, workspace-root
links, and relative paths) and stores workspace-relative canonical paths in CRDT
metadata.

When frontmatter parsing falls back from direct serde mapping, `attachments`
entries are still preserved (string links and object/BinaryRef-style entries),
so attachment refs are not dropped during CRDT metadata updates.

If a file write contains no `attachments` frontmatter at all, `CrdtFs` now
preserves existing CRDT `BinaryRef` entries for that file instead of replacing
them with an empty list.

For ambiguous plain paths (`Folder/file.md`), resolution is hint-aware:

- if workspace/frontmatter link format is `plain_canonical`, ambiguous links are
  treated as workspace-root canonical paths;
- otherwise they default to relative semantics, with a filesystem existence check
  to disambiguate legacy data safely.

## Legacy Rename Handling

`CrdtFs::move_file` now detects legacy path-key CRDT entries (non-UUID keys such
as `notes/file.md`) and uses delete+create semantics for the CRDT mutation:

- Old key is tombstoned.
- New key is created from destination file content.
- Destination body doc is treated as a fresh doc state.

This keeps workspace metadata paths aligned with body-sync doc paths during
renames, which prevents "old path/new path split-brain" sync behavior and
reduces duplicate body merges after rename-heavy sessions.

## Safe-write Recovery

Metadata/frontmatter writes use a temp + backup swap strategy. On OPFS/FSA,
there can be transient races where `exists(path)` is true but moving
`path -> path.bak` returns a `NotFound`/missing-object error.

`metadata_writer` now treats that backup-move race as recoverable and continues
the swap, with a direct-overwrite fallback for transient `NotFound`,
`AlreadyExists`, and browser permission/state write errors. This prevents edit
operations from getting stuck in retry loops during sync churn.
