---
title: diaryx_core src
description: Source code for the core Diaryx library
part_of: "[README](/crates/diaryx_core/README.md)"
contents:
  - "[README](/crates/diaryx_core/src/crdt/README.md)"
  - "[README](/crates/diaryx_core/src/cloud/README.md)"
  - "[README](/crates/diaryx_core/src/entry/README.md)"
  - "[README](/crates/diaryx_core/src/fs/README.md)"
  - "[README](/crates/diaryx_core/src/publish/README.md)"
  - "[README](/crates/diaryx_core/src/utils/README.md)"
  - "[README](/crates/diaryx_core/src/workspace/README.md)"
  - "[README](/crates/diaryx_core/src/import/README.md)"
attachments:
  - "[lib.rs](/crates/diaryx_core/src/lib.rs)"
  - "[backup.rs](/crates/diaryx_core/src/backup.rs)"
  - "[command.rs](/crates/diaryx_core/src/command.rs)"
  - "[command_handler.rs](/crates/diaryx_core/src/command_handler.rs)"
  - "[config.rs](/crates/diaryx_core/src/config.rs)"
  - "[diaryx.rs](/crates/diaryx_core/src/diaryx.rs)"
  - "[error.rs](/crates/diaryx_core/src/error.rs)"
  - "[export.rs](/crates/diaryx_core/src/export.rs)"
  - "[frontmatter.rs](/crates/diaryx_core/src/frontmatter.rs)"
  - "[link_parser.rs](/crates/diaryx_core/src/link_parser.rs)"
  - "[metadata_writer.rs](/crates/diaryx_core/src/metadata_writer.rs)"
  - "[search.rs](/crates/diaryx_core/src/search.rs)"
  - "[template.rs](/crates/diaryx_core/src/template.rs)"
  - "[test_utils.rs](/crates/diaryx_core/src/test_utils.rs)"
  - "[validate.rs](/crates/diaryx_core/src/validate.rs)"
exclude:
  - "*.lock"
---

# diaryx_core Source

This directory contains the source code for the core Diaryx library.

## Structure

| File                 | Purpose                                                |
| -------------------- | ------------------------------------------------------ |
| `lib.rs`             | Library entry point and public API exports             |
| `backup.rs`          | Create ZIP backups of markdown files                   |
| `command.rs`         | Command pattern API for unified WASM/Tauri operations  |
| `command_handler.rs` | Command execution implementation                       |
| `config.rs`          | Configuration management                               |
| `diaryx.rs`          | Central Diaryx data structure                          |
| `error.rs`           | Shared error types                                     |
| `export.rs`          | Export with audience filtering (case-insensitive, trim-aware, no special "private" value) |
| `frontmatter.rs`     | Frontmatter parsing and manipulation                   |
| `link_parser.rs`     | Parse markdown links                                   |
| `metadata_writer.rs` | Write frontmatter metadata (temp + backup safe writes) |
| `search.rs`          | Search functionality                                   |
| `template.rs`        | Template management                                    |
| `test_utils.rs`      | Feature-gated test utilities                           |
| `validate.rs`        | Workspace validation and fixing                        |

## SetFrontmatterProperty: Atomic Title Rename

When `SetFrontmatterProperty` is called with `key="title"` and a `root_index_path`,
the handler reads workspace config and atomically:
1. Computes the new filename using `apply_filename_style()` (if `auto_rename_to_title` is enabled)
2. Renames the file via `workspace.rename_entry()` (handles both leaf files and index directories)
3. Migrates the body CRDT doc to the new path
4. Sets the title in frontmatter at the (possibly new) path
5. Syncs the first H1 heading (if `sync_title_to_heading` is enabled)

Returns `Response::String(new_path)` if a rename occurred, `Response::Ok` otherwise.

## CRDT Metadata Notes

- `Command::SetCrdtFile` preserves existing attachment `BinaryRef` metadata
  when incoming metadata omits attachments or includes refs with empty hashes.
  This avoids dropping cloud attachment references during frontmatter-driven
  metadata refreshes.

## Attachment Path Resolution Notes

- `command_handler.rs` resolves attachment storage paths via shared normalization
  for markdown links, root-relative refs, plain relative refs, and plain
  canonical refs that include the current entry directory. This keeps
  get/delete/move attachment commands consistent for nested entries.

## TypeScript Binding Notes

- `ts-rs` is configured with `no-serde-warnings` in `Cargo.toml`.
- For fields that are conditionally omitted by serde (for example via
  `skip_serializing_if`), prefer explicit `#[ts(...)]` annotations (such as
  `#[ts(optional)]` or `#[ts(type = "...")]`) to keep TS output aligned with
  runtime JSON shape.
