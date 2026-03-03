---
title: diaryx_publish
description: Publishing pipeline for Diaryx workspaces — converts markdown to HTML
part_of: '[crates](/crates/README.md)'
attachments:
  - '[Cargo.toml](/crates/diaryx_publish/Cargo.toml)'
  - '[lib.rs](/crates/diaryx_publish/src/lib.rs)'
  - '[publisher.rs](/crates/diaryx_publish/src/publisher.rs)'
  - '[types.rs](/crates/diaryx_publish/src/types.rs)'
  - '[fs_content_provider.rs](/crates/diaryx_publish/src/fs_content_provider.rs)'
exclude:
  - '*.lock'
---
# diaryx_publish

Publishing pipeline that converts Diaryx workspace markdown files to HTML.

## Overview

This crate owns the full markdown-to-HTML rendering pipeline using [comrak](https://docs.rs/comrak). It was extracted from `diaryx_core::publish` to allow publishing to work independently of sync/CRDT infrastructure.

The `ContentProvider` trait (defined in `diaryx_core`) abstracts where content comes from — filesystem, CRDT state, etc. This crate provides `FilesystemContentProvider` for reading directly from local files.

## Key Types

- `Publisher` — main entry point; takes an `AsyncFileSystem` and publishes a workspace to a destination directory
- `PublishOptions` — configuration (single-file mode, audience filter, title, force overwrite, attachment copying)
- `PublishResult` / `PublishedPage` — output metadata
- `FilesystemContentProvider` — reads workspace content from the local filesystem
- `ContentProvider` / `MaterializedFile` — re-exported from `diaryx_core` for convenience

## Features

- Multi-file and single-file HTML output modes
- Audience filtering via the export system
- Automatic navigation links from `contents`/`part_of` frontmatter
- Floating metadata pill showing frontmatter key-value pairs
- **Footnotes**: Comrak's built-in footnote extension (`[^label]` / `[^label]: ...`)
- **Colored highlights**: `==text==` and `=={color}text==` syntax with 10-color palette
- **Spoilers**: `||text||` click-to-reveal elements
- **Attachment copying**: Referenced files in `_attachments/` directories are discovered and copied

## Cargo Features

- `markdown` (default) — compatibility flag, no-op (comrak is always included)
- `templating` — enables render-time body templating (passes through to `diaryx_core/templating`)

## Dependencies

- `diaryx_core` — shared kernel (workspace, entry, filesystem, ContentProvider trait)
- `comrak` — markdown-to-HTML rendering
