---
title: Utils module
description: Utility functions for date and path handling
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/utils/mod.rs)'
  - '[date.rs](/crates/diaryx_core/src/utils/date.rs)'
  - '[path.rs](/crates/diaryx_core/src/utils/path.rs)'
exclude:
  - '*.lock'
---

# Utils Module

Utility functions for date parsing and path manipulation.

## Files

- `mod.rs` - Module exports
- `date.rs` - Natural language date parsing with chrono
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
