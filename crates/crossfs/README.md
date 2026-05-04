---
title: crossfs
description: std::fs / tokio::fs that also runs on OPFS, IndexedDB, and the File System Access API in the browser
part_of: '[crates](/crates/README.md)'
exclude:
  - '*.lock'
  - '**/*.rs'
---

# crossfs

A cross-platform filesystem abstraction for Rust. Mirrors the API of
`std::fs` / `tokio::fs` and adds first-class support for browser storage:

- **`StdFs`** — backed by `std::fs` (native).
- **`TokioFs`** — backed by `tokio::fs` (planned, v0.2).
- **`InMemoryFs`** — portable in-memory filesystem (tests, sandboxing).
- **`OpfsFs`** — Origin Private File System (browser).
- **`IndexedDbFs`** — IndexedDB-backed (browser).
- **`FsaFs`** — File System Access API (user-picked directories, browser).
- **`JsAsyncFs`** — bring-your-own JS callback bridge (Node/Electron/etc.).

## Status

Pre-0.1, in-tree only inside the Diaryx workspace. The API is being
extracted from `diaryx_core::fs` and aligned with `std::fs` naming. Not
yet published to crates.io.

## Why not `vfs`?

The `vfs` crate is positioned for embedded assets and overlay filesystems
(zip, embedded-fs). Its async support is being sunset, which is the wrong
direction for OPFS / IndexedDB — those are inherently async, persistent
browser storage.

## Feature matrix

| Feature      | Pulls in                                        | Default |
|--------------|-------------------------------------------------|---------|
| `std`        | (no extra deps)                                 | yes     |
| `tokio`      | `tokio` (`fs` feature)                          | no      |
| `sha256`     | `sha2`                                          | no      |
| `js`         | `wasm-bindgen`, `js-sys`, `wasm-bindgen-futures`| no      |
| `opfs`       | `js` + `opfs` + `web-sys`                       | no      |
| `indexeddb`  | `js` + `indexed-db`                             | no      |
| `fsa`        | `js` + `opfs` + `web-sys`                       | no      |

Browser features are additionally target-gated to `wasm32`.

## Known gaps

- No `File` / `OpenOptions` / streaming I/O yet (planned, v0.2). All I/O
  is whole-buffer in v0.1, matching `tokio::fs`'s original release.
- No `copy`, `canonicalize`, `hard_link`, `set_permissions`. Add per
  request.

## License

MIT OR Apache-2.0.
