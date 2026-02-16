---
title: diaryx_apple
description: UniFFI bridge crate for Apple clients
author: adammharris
audience:
- developers
part_of: '[README](/crates/README.md)'
contents:
  - '[README](/crates/diaryx_apple/src/README.md)'
attachments:
  - '[Cargo.toml](/crates/diaryx_apple/Cargo.toml)'
exclude:
  - '*.lock'
---

# diaryx_apple

`diaryx_apple` is the Apple-facing bridge crate for `diaryx_core`.

It exposes a small UniFFI-friendly API to support incremental migration of `apps/apple` from direct filesystem operations to Rust core logic.

## Initial API Surface

- `open_workspace(path)`
- `DiaryxAppleWorkspace.list_entries()`
- `DiaryxAppleWorkspace.get_entry(id)`
- `DiaryxAppleWorkspace.save_entry(id, markdown)`

All entry IDs are currently workspace-relative markdown paths.

## Notes

- This crate intentionally starts thin and coarse-grained.
- The first version returns raw markdown (including frontmatter) for compatibility with the current Swift editor flow.
- Future iterations can move more behavior (validation, tree traversal, search, sync) behind this boundary.
