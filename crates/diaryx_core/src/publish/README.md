---
title: Publish module
description: HTML publishing using comrak
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/publish/mod.rs)'
  - '[types.rs](/crates/diaryx_core/src/publish/types.rs)'
exclude:
  - '*.lock'
---

# Publish Module

This module converts markdown files to HTML using [comrak](https://docs.rs/comrak).

## Files

- `mod.rs` - Publisher implementation with TOC generation and syntax highlighting
- `types.rs` - PublishOptions and related types
