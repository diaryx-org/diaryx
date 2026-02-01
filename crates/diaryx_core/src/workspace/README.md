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
