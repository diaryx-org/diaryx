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

## API Surface

- `open_workspace(path)`
- `DiaryxAppleWorkspace.list_entries()` → `Vec<EntrySummary>`
- `DiaryxAppleWorkspace.get_entry(id)` → `EntryData` (includes `body`, `metadata`, and raw `markdown`)
- `DiaryxAppleWorkspace.save_entry(id, markdown)` — save raw markdown (including frontmatter)
- `DiaryxAppleWorkspace.save_entry_body(id, body)` — save body only, preserving existing frontmatter

All entry IDs are currently workspace-relative markdown paths.

### Records

- `EntrySummary` — `id`, `path`, `title`
- `EntryData` — `id`, `path`, `markdown` (raw), `body` (without frontmatter), `metadata` (parsed fields)
- `MetadataField` — `key`, `value` (scalar string), `values` (array items)

## Generating Swift Bindings

The crate includes a `uniffi-bindgen` binary (gated behind the `bindgen` feature) for generating Swift source and C headers:

```bash
# Build the static library
cargo build -p diaryx_apple --target aarch64-apple-darwin --release

# Generate Swift bindings from the built library
cargo run -p diaryx_apple --features bindgen --bin uniffi-bindgen -- \
    generate --library target/aarch64-apple-darwin/release/libdiaryx_apple.a \
    --language swift --out-dir out/
```

This produces `diaryx_apple.swift`, `diaryx_appleFFI.h`, and `diaryx_appleFFI.modulemap`.

The `apps/apple/build-rust.sh` script automates this and packages the output into an XCFramework.

## Notes

- This crate intentionally starts thin and coarse-grained.
- `get_entry()` uses `diaryx_core::frontmatter::parse_or_empty()` to split content into body and metadata.
- `save_entry_body()` reads the existing file, preserves frontmatter, and writes back with the new body.
- Future iterations can move more behavior (validation, tree traversal, search, sync) behind this boundary.
