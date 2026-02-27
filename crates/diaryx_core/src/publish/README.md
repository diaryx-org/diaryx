---
title: Publish module
description: ContentProvider trait — shared publish abstractions
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/publish/mod.rs)'
  - '[content_provider.rs](/crates/diaryx_core/src/publish/content_provider.rs)'
exclude:
  - '*.lock'
---

# Publish Module (Core)

This module defines the shared abstractions for publishing. The full rendering pipeline (markdown-to-HTML via comrak) has moved to the [`diaryx_publish`](/crates/diaryx_publish/README.md) crate.

## What remains here

- `ContentProvider` trait — abstraction for content sources (filesystem, CRDT, etc.)
- `MaterializedFile` — a file ready for rendering (path, content, frontmatter)

These types live in `diaryx_core` because they are part of the shared kernel used by both the publish crate and potential server-side content providers.

## Files

- `mod.rs` — re-exports from `content_provider`
- `content_provider.rs` — `ContentProvider` trait and `MaterializedFile` type
