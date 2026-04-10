---
title: diaryx_core src
description: Source code for the core Diaryx library
part_of: '[README](/crates/diaryx_core/README.md)'
contents:
- '[README](/crates/diaryx_core/src/entry/README.md)'
- '[README](/crates/diaryx_core/src/fs/README.md)'
- '[README](/crates/diaryx_core/src/plugin/README.md)'
- '[README](/crates/diaryx_core/src/utils/README.md)'
- '[README](/crates/diaryx_core/src/workspace/README.md)'
- '[README](/crates/diaryx_core/src/import/README.md)'
exclude:
- '*.lock'
- '**/*.rs'
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
| `validate/` | Workspace validation and auto-fixing (split by concern)     |
| `workspace/`| Workspace tree organization                                 |
| `import/`   | Import external formats                                     |

### `validate/` submodules

| File            | Purpose                                                             |
| --------------- | ------------------------------------------------------------------- |
| `mod.rs`        | Thin re-exporting shim                                              |
| `types.rs`      | `ValidationError`, `ValidationWarning`, `ValidationResult` + metadata impls (`description`, `can_auto_fix`, `file_path`, `is_viewable`, `supports_parent_picker`) |
| `meta.rs`       | `*WithMeta` wrappers that attach `description`, `detail`, `primary_path`, and UI booleans for consumers |
| `detail.rs`     | Per-variant contextual one-line summaries (`warning.detail()` / `error.detail()`) used by CLI output and by the `WithMeta` wrappers |
| `check.rs`      | Pure helpers (portability checks, canonical-link equivalence, duplicate detection, single-index finder) |
| `validator.rs`  | Async `Validator` that walks a workspace and emits warnings/errors  |
| `fixer.rs`      | Async `ValidationFixer` + `FixResult`; dispatch hub is `fix_warning` / `fix_error` so callers never switch on variants |
| `tests.rs`      | Test suite exercising the public re-exported surface               |

Adding a new `ValidationWarning` variant requires touching (a) the enum + its metadata impls in `types.rs`, (b) a detail arm in `detail.rs`, (c) the validator logic that emits it in `validator.rs`, and (d) a dispatch arm in `fixer.rs::fix_warning`. The CLI and frontend are variant-agnostic and do not need updating.

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
- Entry `attachments[]` frontmatter now stores links to attachment notes rather
  than direct binary paths. Each binary is represented by a sibling note such
  as `_attachments/photo.png.md`, whose frontmatter carries `attachment` (the
  binary self-link) and `attachment_of` backlinks.
- Attachment uploads now split into two steps: callers call
  `Command::RegisterAttachment` first (which creates the attachment note and
  returns `Strings([link, storage_path])`), then write raw bytes to the returned
  storage path through the filesystem API. This keeps large media off the
  JSON/base64 command path while preserving one canonical link-formatting
  implementation in Rust and letting the backend own path resolution.
- `GetAttachmentData`, `ResolveAttachmentPath`, `DeleteAttachment`, and
  `MoveAttachment` all resolve through the attachment note. Delete only removes
  the binary/note pair when the final `attachment_of` backlink is gone, and
  move updates backlinking entries to the new attachment-note path. Read/preview
  flows still accept direct binary body refs (for example raw HTML `<img src>`
  and `<picture><source srcset>` paths) so existing body media does not need to
  become note-backed before it can render.
- Validation follows attachment notes: when an index lists an attachment note
  in `attachments`, the validator parses the note and marks the binary that its
  `attachment:` property points at as visited, so the orphan scanner never
  flags note-wrapped binaries. A missing `attachments` target (note or binary)
  still produces a `BrokenAttachment` error. The `fix_orphan_binary_file` fix
  creates a sibling attachment note next to the binary and links the note
  (not the binary) into the index's `attachments` list.
- An `attachments` entry that points directly at a binary (legacy flat format)
  produces an `InvalidAttachmentRef` warning tagged
  `InvalidAttachmentRefKind::LegacyBinary`, which is auto-fixable via
  `ValidationFixer::fix_invalid_attachment_ref`: the fix wraps the binary in a
  sibling attachment note and rewrites the source index's `attachments` entry
  to point at the new note. The `NotAttachmentNote` and `UnparseableNote`
  kinds remain non-auto-fixable because the intended binary is ambiguous.
- The singular attachment-note `attachment` property is also normalized through
  the attachment-aware link pipeline, so plain canonical binary paths are read
  correctly and rewritten back into the workspace link format on save.
- Frontmatter attachment values set through `SetFrontmatterProperty` and
  `ConvertLinks` are normalized through the same link parser pipeline used for
  `part_of`/`contents`, then formatted according to workspace `link_format`.
- The singular `link` frontmatter property is normalized through the same path
  too, and validation treats it as the file's canonical self-link.
- `links` and `link_of` frontmatter arrays now use the same normalization and
  conversion pipeline, and validation/fixer logic treats them as an explicit
  outbound/backlink graph layered on top of the main `contents` / `part_of`
  hierarchy.
- `attachments` and `attachment_of` get the same backlink audit as
  `links`/`link_of`. An index that lists an attachment note whose
  `attachment_of` does not point back produces a `MissingAttachmentBacklink`
  warning; an attachment note with a stale `attachment_of` entry (missing
  source, or source no longer lists the note) produces
  `StaleAttachmentBacklink`. Both are auto-fixable via
  `ValidationFixer::fix_missing_attachment_backlink` /
  `fix_stale_attachment_backlink`.
- Orphan-file/orphan-binary exclude patterns are inherited from the nearest
  actual index file and its `part_of` ancestors, even when sibling leaf
  markdown files are present in the same directory as the index.
- Validation matches exclude patterns against both basenames and
  workspace-relative paths, and now prunes excluded files/directories during
  the orphan scan instead of walking them first and suppressing warnings later.
- Validation also prunes hidden dot-directories before recursion, so local tool
  caches like `.direnv`, `.zig-cache`, and `.diaryx` do not inflate monorepo
  orphan scans.
- Validation also prunes common build/dependency directories such as `target`,
  `node_modules`, `dist`, `build`, and `.git` before recursing into them, so
  large monorepo artifacts do not dominate orphan-scan startup time.
- Filesystem-tree mode (`GetFilesystemTree`, used by "Show All Files") now uses
  the same basename + workspace-relative exclude matching, inherits excludes
  from the nearest nested index and its `part_of` ancestors, and prunes the
  same built-in non-workspace directories before recursion.
- Validation and filesystem-tree scans now also emit log summaries with the
  number of directories explored/pruned, plus debug-level directory lists for
  tracing monorepo-specific slow paths.
- Export binary attachment enumeration now derives from the logical workspace
  file set (`collect_workspace_file_set`) rather than a raw filesystem walk, so
  it stays scoped to reachable workspace entries and attachments.

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
- `Command::AddLink` / `Command::RemoveLink` manage the explicit non-structural
  link graph in frontmatter. `AddLink` ensures `source.links`, `target.link_of`,
  and a missing target `link` self-reference are populated without duplicates.
  `RemoveLink` accepts an optional current body snapshot and only removes the
  relation when no matching local markdown link remains in the source body.

## TypeScript Binding Notes

- `ts-rs` is configured with `no-serde-warnings` in `Cargo.toml`.
- For fields that are conditionally omitted by serde (for example via
  `skip_serializing_if`), prefer explicit `#[ts(...)]` annotations (such as
  `#[ts(optional)]` or `#[ts(type = "...")]`) to keep TS output aligned with
  runtime JSON shape.
