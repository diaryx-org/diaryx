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

- `mod.rs` - Workspace implementation with tree building, `WorkspaceConfig`, `FilenameStyle`
- `types.rs` - TreeNode, IndexFrontmatter, and audience visibility logic

## WorkspaceConfig

Workspace-level configuration lives in the root index file's YAML frontmatter `extra` fields. `get_workspace_config()` extracts these into a `WorkspaceConfig` struct, and `set_workspace_config_field()` writes them back.

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `link_format` | `LinkFormat` | `MarkdownRoot` | How `part_of`/`contents` links are formatted |
| `daily_entry_folder` | `Option<String>` | `None` | Subfolder for daily entries |
| `default_template` | `Option<String>` | `None` | Link to default template entry |
| `daily_template` | `Option<String>` | `None` | Link to daily template entry |
| `sync_title_to_heading` | `bool` | `false` | Update first H1 when title changes |
| `auto_update_timestamp` | `bool` | `true` | Auto-set `updated` on save |
| `auto_rename_to_title` | `bool` | `true` | Auto-rename file when title changes |
| `filename_style` | `FilenameStyle` | `Preserve` | How titles map to filenames |
| `public_audience` | `Option<String>` | `None` | Audience tag for publishable entries |

## FilenameStyle

Controls how entry titles are converted to filenames:

- `Preserve` (default) - Strip only filesystem-illegal characters, keep spaces/caps/unicode
- `KebabCase` - Lowercase with hyphens (e.g., `my-note-title`)
- `SnakeCase` - Lowercase with underscores (e.g., `my_note_title`)
- `ScreamingSnakeCase` - Uppercase with underscores (e.g., `MY_NOTE_TITLE`)

## Audience Visibility

`IndexFrontmatter.is_visible_to(audience)` checks whether an entry should be included for a given audience. There is no special "private" value -- all audience tags are treated equally. The `public_audience` workspace config field designates which audience tag means "publishable" for the publish pipeline.

## Rename/Move Consistency

Workspace rename/move operations now prefer non-lossy index updates:

- Parent `contents` updates add the new canonical reference before removing the old one.
- Same-parent renames skip unnecessary `part_of` rewrites.
- Index renames update `part_of` for children discovered from the index `contents`
  list (including nested child paths), instead of only same-directory markdown files.
- Cleanup failures when removing old `contents` references are logged as warnings instead of silently ignored.

This reduces transient states where a renamed child disappears from workspace trees.

## Non-Portable Filename Validation

Validation detects filenames containing characters that are not portable across platforms. Chrome's File System Access API (used by the web frontend) rejects these characters even on macOS/Linux, applying Windows-level restrictions:

- **Anywhere in filename:** `"`, `*`, `/`, `\`, `:`, `<`, `>`, `?`, `|`, control characters (U+0000-U+001F, U+007F)
- **At start/end of filename stem:** `.`, `~`, whitespace

Files with non-portable filenames produce a `NonPortableFilename` warning with a suggested sanitized filename. The `Preserve` filename style also strips these characters when generating new filenames.

## Duplicate Contents Entries

Tree building now deduplicates child paths per index when `contents` includes
the same target more than once. This prevents duplicate `TreeNode.path` values
from propagating to clients and avoids keyed-render crashes in UIs.
