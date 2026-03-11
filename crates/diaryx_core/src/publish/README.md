---
title: Publish module
description: Format-agnostic publishing pipeline
part_of: '[README](/crates/diaryx_core/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_core/src/publish/mod.rs)'
  - '[content_provider.rs](/crates/diaryx_core/src/publish/content_provider.rs)'
  - '[publisher.rs](/crates/diaryx_core/src/publish/publisher.rs)'
  - '[publish_format.rs](/crates/diaryx_core/src/publish/publish_format.rs)'
  - '[body_renderer.rs](/crates/diaryx_core/src/publish/body_renderer.rs)'
  - '[types.rs](/crates/diaryx_core/src/publish/types.rs)'
  - '[fs_content_provider.rs](/crates/diaryx_core/src/publish/fs_content_provider.rs)'
  - '[html_format.rs](/crates/diaryx_core/src/publish/html_format.rs)'
exclude:
  - '*.lock'
---

# Publish Module

Format-agnostic workspace publishing pipeline. `Publisher` orchestrates workspace file collection, navigation resolution, body template rendering, and delegates format-specific work to a `PublishFormat` implementation.

## Key types

- `Publisher<FS>` — async workspace publisher, generic over filesystem
- `PublishFormat` trait — format-specific behavior (body conversion, link rewriting, page rendering)
- `HtmlFormat` — comrak-backed HTML format (behind `html-publish` feature flag)
- `BodyRenderer` trait — template rendering abstraction; `NoopBodyRenderer` for passthrough
- `ContentProvider` trait — content source abstraction (filesystem, CRDT, etc.)
- `FilesystemContentProvider` — reads workspace files from local filesystem
- `PublishOptions`, `PublishResult`, `PublishedPage`, `NavLink` — data types

## Feature flags

- `html-publish` — enables `HtmlFormat` and the `comrak` dependency

## Files

- `mod.rs` — module wiring and re-exports
- `publisher.rs` — `Publisher` orchestrator
- `publish_format.rs` — `PublishFormat` trait
- `html_format.rs` — `HtmlFormat` impl (gated by `html-publish`)
- `html_format_css.css` — built-in stylesheet for HTML output
- `body_renderer.rs` — `BodyRenderer` trait + `NoopBodyRenderer`
- `types.rs` — `PublishOptions`, `PublishResult`, `PublishedPage`, `NavLink`
- `content_provider.rs` — `ContentProvider` trait + `MaterializedFile`
- `fs_content_provider.rs` — `FilesystemContentProvider`
