---
title: diaryx_native
description: Native (non-WASM) filesystem, config, and blocking adapters for diaryx_core
author: adammharris
part_of: '[README](/crates/README.md)'
exclude:
- '*.lock'
- '**/*.rs'
---
# diaryx_native

Native-platform adapters for [`diaryx_core`](../diaryx_core/README.md).

This crate mirrors the role of [`diaryx_wasm`](../diaryx_wasm/README.md): it is the home for platform-specific `FileSystem` implementations and platform conventions that don't belong in the cross-platform core.

## What's in here

| Item                          | Source                         | Purpose                                                                             |
| ----------------------------- | ------------------------------ | ----------------------------------------------------------------------------------- |
| `RealFileSystem`              | `src/fs.rs`                    | `std::fs`-backed `diaryx_core::fs::FileSystem` implementation                       |
| `default_config()`            | `src/config.rs`                | Build a `Config` with `default_workspace = ~/diaryx` (via `dirs`)                   |
| `config_path()`               | `src/config.rs`                | Resolve `~/.config/diaryx/config.md` (via `dirs`)                                   |
| `NativeConfigExt` trait       | `src/config.rs`                | Restores the `Config::load()` / `save()` / `init()` / `_sync` wrappers as methods   |
| `block_on`                    | `src/lib.rs` (re-export)       | `futures_lite::future::block_on`, so native callers don't each import futures-lite  |

## Why this crate exists

`diaryx_core` is the portable data/logic layer — it must compile unchanged for WASM (`diaryx_wasm`, browser) and for Extism guest plugins (wasmtime, no JS). Previously it carried three dependencies that only make sense on native hosts:

- `dirs` — used to resolve `~/.config/diaryx/config.md` and `~/diaryx`
- `futures-lite` — used by the `_sync` wrappers (`block_on`)
- `rusqlite` (optional, behind the `database` feature) — used only to derive `From<rusqlite::Error> for DiaryxError`

All three were already `cfg(not(target_arch = "wasm32"))`-gated, so they never appeared in the WASM binary. Keeping them in `diaryx_core`'s `Cargo.toml` only muddied the dependency graph. Moving the native-only code here (and replacing `DiaryxError::Database(rusqlite::Error)` with `DiaryxError::Database(String)` downstream) gives `diaryx_core` a clean, portable dependency list.

## Usage

```rust,ignore
use diaryx_core::config::Config;
use diaryx_core::fs::SyncToAsyncFs;
use diaryx_core::workspace::Workspace;
use diaryx_native::{NativeConfigExt, RealFileSystem, block_on};

// Platform-specific filesystem, wrapped for the async-first core APIs.
let async_fs = SyncToAsyncFs::new(RealFileSystem);
let workspace = Workspace::new(async_fs);

// Load the user's config from ~/.config/diaryx/config.md
// (or fall back to ~/diaryx). Requires NativeConfigExt in scope.
let cfg = Config::load()?;

// Blocking helpers for sync contexts (CLI, tests, Tauri commands).
let tree = block_on(workspace.build_tree(std::path::Path::new("README.md")))?;
# Ok::<(), diaryx_core::error::DiaryxError>(())
```

## Relationship to `diaryx_wasm`

| Role                         | Native crate                              | WASM crate                                         |
| ---------------------------- | ----------------------------------------- | -------------------------------------------------- |
| Provides `FileSystem` impl   | `diaryx_native::RealFileSystem`           | backends in `diaryx_wasm` (OPFS / IndexedDB / FSA) |
| Resolves config path         | `diaryx_native::config_path()` (via dirs) | host-provided virtual path                         |
| Blocking adapter             | `diaryx_native::block_on` (futures-lite)  | n/a (browser I/O is inherently async)              |

## Who depends on this

- `diaryx` (CLI)
- `diaryx_tauri` (desktop + mobile)
- `diaryx_extism` (Extism host — uses `RealFileSystem` when loading plugins from disk)
- `xtask` (workspace tooling)
- `diaryx_sync` (dev-dep only, for integration tests)
