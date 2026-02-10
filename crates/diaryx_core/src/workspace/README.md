---
title: Workspace module
description: Workspace tree organization
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/workspace/mod.rs)'
  - '[types.rs](/crates/diaryx_core/src/workspace/types.rs)'
exclude:
  - '*.lock'
---

# Workspace Module

This module organizes collections of markdown files into hierarchical workspaces using `part_of` and `contents` relationships.

## Files

- `mod.rs` - Workspace implementation with tree building
- `types.rs` - TreeNode and related types

## Rename/Move Consistency

Workspace rename/move operations now prefer non-lossy index updates:

- Parent `contents` updates add the new canonical reference before removing the old one.
- Same-parent renames skip unnecessary `part_of` rewrites.
- Cleanup failures when removing old `contents` references are logged as warnings instead of silently ignored.

This reduces transient states where a renamed child disappears from workspace trees.
