---
title: Utils module
description: Utility functions for date and path handling
part_of: '[README](/crates/diaryx_core/src/README.md)'
exclude:
  - '*.lock'
  - '**/*.rs'
---

# Utils Module

Utility functions for date parsing and path manipulation.

## Files

- `mod.rs` - Module exports
- `date.rs` - Natural language date parsing with chrono
- `naming.rs` - Workspace naming, URL normalization, and publishing slug validation
- `path.rs` - Path utilities (relative paths, normalization)

## Sync Path Canonicalization

`path.rs` includes `normalize_sync_path(path: &str) -> String`, a shared
canonicalization helper used by sync-critical modules.

Behavior:

- Strips leading `./` and `/`
- Normalizes separators to `/`
- Preserves nested relative structure

This makes sync keys stable across path aliases (for example `README.md`,
`./README.md`, and `/README.md`) and avoids duplicate or missed sync state for
the same logical file.

`path.rs` also includes
`strip_workspace_root_prefix(path: &str, workspace_root: &Path) -> Option<String>`.
This helper strips workspace-root prefixes from absolute paths and from the
corrupted absolute form where the leading slash is missing (for example
`Users/alice/workspace/README.md`), returning workspace-relative paths when a
prefix match is found.
