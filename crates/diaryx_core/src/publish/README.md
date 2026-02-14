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

## Features

- Multi-file and single-file HTML output modes
- Audience filtering via the export system
- Automatic navigation links from `contents`/`part_of` frontmatter
- Floating metadata pill showing frontmatter key-value pairs
- **Attachment copying**: Referenced files in `_attachments/` directories are discovered from markdown body links/images and the `attachments` frontmatter list, then copied to the output directory preserving relative paths. Enabled by default; disable with `PublishOptions::copy_attachments = false` (or `--no-copy-attachments` in the CLI). Skipped in single-file mode and by the sync server (which uses R2 URL rewriting instead).

## Files

- `mod.rs` - Publisher implementation with attachment discovery, TOC generation, and HTML rendering
- `types.rs` - PublishOptions, PublishResult, and related types
