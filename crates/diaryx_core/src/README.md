---
title: diaryx_core src
description: Source code for the core Diaryx library
part_of: "[README](/crates/diaryx_core/README.md)"
contents:
  - "[README](/crates/diaryx_core/src/crdt/README.md)"
  - "[README](/crates/diaryx_core/src/cloud/README.md)"
  - "[README](/crates/diaryx_core/src/entry/README.md)"
  - "[README](/crates/diaryx_core/src/fs/README.md)"
  - "[README](/crates/diaryx_core/src/plugin/README.md)"
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
  - "[body_template.rs](/crates/diaryx_core/src/body_template.rs)"
  - "[template.rs](/crates/diaryx_core/src/template.rs)"
  - "[test_utils.rs](/crates/diaryx_core/src/test_utils.rs)"
  - "[validate.rs](/crates/diaryx_core/src/validate.rs)"
  - "[workspace_registry.rs](/crates/diaryx_core/src/workspace_registry.rs)"
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
| `body_template.rs`   | Render-time body templating via Handlebars (`templating` feature) |
| `template.rs`        | Creation-time template management                      |
| `test_utils.rs`      | Feature-gated test utilities                           |
| `validate.rs`        | Workspace validation and fixing                        |
| `workspace_registry.rs` | Multi-workspace registry types (shared across frontends) |

## Modules

| Directory   | Purpose                                                     |
| ----------- | ----------------------------------------------------------- |
| `plugin/`   | Plugin architecture: traits, events, registry               |
| `publish/`  | HTML publishing pipeline (includes `ContentProvider` trait)  |
| `crdt/`     | CRDT sync and version history (requires `crdt` feature)     |
| `cloud/`    | Bidirectional file sync with cloud storage                  |
| `entry/`    | Entry manipulation functionality                            |
| `fs/`       | Filesystem abstraction layer                                |
| `utils/`    | Utility functions (date, path)                              |
| `workspace/`| Workspace tree organization                                 |
| `import/`   | Import external formats                                     |

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
- Attachment uploads now split into two steps: callers write raw bytes through
  the filesystem API, then `Command::RegisterAttachment` formats and records the
  attachment link in entry frontmatter. This keeps large media off the
  JSON/base64 command path while preserving one canonical link-formatting
  implementation in Rust.
- Frontmatter attachment values set through `SetFrontmatterProperty` and
  `ConvertLinks` are normalized through the same link parser pipeline used for
  `part_of`/`contents`, then formatted according to workspace `link_format`.
- The singular `link` frontmatter property is normalized through the same path
  too, and validation treats it as the file's canonical self-link.
- `links` and `link_of` frontmatter arrays now use the same normalization and
  conversion pipeline, and validation/fixer logic treats them as an explicit
  outbound/backlink graph layered on top of the main `contents` / `part_of`
  hierarchy.

## Command Path Resolution Notes

- `execute()` normalizes command path fields to workspace-relative values when a
  workspace root is configured.
- CRDT commands with `doc_name` path semantics (for body docs and generic
  sync-doc operations) are normalized through the same workspace-root stripping
  logic, so absolute paths do not leak into sync doc IDs.
- Workspace-root stripping also handles the corrupted absolute form where a
  leading slash was already removed (for example `Users/.../workspace/file.md`).
- Handlers that call `Workspace`, `Validator`, exporter/search root APIs, or
  raw filesystem methods must resolve those normalized values back to
  filesystem paths via `resolve_fs_path(...)` before reading/writing disk.
- This preserves cross-platform behavior for Tauri absolute inputs while
  keeping canonical workspace-relative command semantics for sync/link logic.
- External metadata sync now resolves move destinations using the nearest
  ancestor index (not just the immediate directory) and clears stale `part_of`
  when no destination index exists, so `contents`/`part_of` stay consistent
  after external moves into unindexed folders.
- `Command::RemoveWorkspacePluginData` owns uninstall-time cleanup for the root
  index's `plugins.<id>` entry and matching `disabled_plugins` entry, so
  frontends do not need to hand-edit workspace frontmatter when removing a
  plugin.

## TypeScript Binding Notes

- `ts-rs` is configured with `no-serde-warnings` in `Cargo.toml`.
- For fields that are conditionally omitted by serde (for example via
  `skip_serializing_if`), prefer explicit `#[ts(...)]` annotations (such as
  `#[ts(optional)]` or `#[ts(type = "...")]`) to keep TS output aligned with
  runtime JSON shape.
