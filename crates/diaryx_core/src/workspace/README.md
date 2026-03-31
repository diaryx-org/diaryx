---
title: Workspace module
description: Workspace tree organization
part_of: '[README](/crates/diaryx_core/src/README.md)'
exclude:
  - '*.lock'
  - '**/*.rs'
---

# Workspace Module

This module organizes collections of markdown files into hierarchical workspaces using `part_of` and `contents` relationships, with optional canonical self-links via `link` and explicit non-structural link graphs via `links` / `link_of`.

## Files

- `mod.rs` - Workspace implementation with tree building, `WorkspaceConfig`, `FilenameStyle`
- `types.rs` - TreeNode, IndexFrontmatter, and audience visibility logic

## WorkspaceConfig

Workspace-level configuration lives in the root index file's YAML frontmatter `extra` fields. `get_workspace_config()` extracts these into a `WorkspaceConfig` struct, and `set_workspace_config_field()` writes them back.

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `link_format` | `LinkFormat` | `MarkdownRoot` | How `link`/`links`/`link_of`/`part_of`/`contents`/`attachments` links are formatted |

## Canonical Self-Links

Entries may declare a singular `link` frontmatter property that represents the
canonical way to refer to the file. Diaryx normalizes `link` values through the
same link parser used for `part_of` and `contents`, and validation warns when a
declared `link` does not resolve back to the file itself.

Files may also declare `links` and `link_of` arrays for explicit outbound and
inbound link relationships. Validation reports broken outbound targets as
errors, warns when a target is missing the expected backlink, and warns when a
stored backlink is stale.
| `default_template` | `Option<String>` | `None` | Link to default template entry |
| `sync_title_to_heading` | `bool` | `false` | Update first H1 when title changes |
| `auto_update_timestamp` | `bool` | `true` | Auto-set `updated` on save |
| `auto_rename_to_title` | `bool` | `true` | Auto-rename file when title changes |
| `filename_style` | `FilenameStyle` | `Preserve` | How titles map to filenames |
| `default_audience` | `Option<String>` | `None` | Audience tag assigned to entries with no explicit/inherited audience. Unset = private. |
| `daily_entry_folder` | `Option<String>` | `None` | Folder used by the Daily plugin for date-based entries. |
| `theme_mode` | `Option<String>` | `None` | Workspace theme mode preference (`light`, `dark`, `system`). |
| `theme_preset` | `Option<String>` | `None` | Active workspace theme preset ID. |
| `theme_accent_hue` | `Option<f64>` | `None` | Accent hue override for the active workspace theme. |

## FilenameStyle

Controls how entry titles are converted to filenames:

- `Preserve` (default) - Strip only filesystem-illegal characters, keep spaces/caps/unicode
- `KebabCase` - Lowercase with hyphens (e.g., `my-note-title`)
- `SnakeCase` - Lowercase with underscores (e.g., `my_note_title`)
- `ScreamingSnakeCase` - Uppercase with underscores (e.g., `MY_NOTE_TITLE`)

## Audience Visibility

`IndexFrontmatter.is_visible_to(audience)` checks whether an entry should be included for a given audience. There is no special "private" value -- all audience tags are treated equally. The `default_audience` workspace config field assigns a default audience to entries with no explicit or inherited audience tag. When unset, unconstrained entries are private (excluded from exports).

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

## Filesystem Tree Mode

`build_filesystem_tree*()` powers "Show All Files" mode. It now:

- matches `exclude` patterns against both basenames and workspace-relative paths
- inherits excludes from the nearest nested index and its `part_of` ancestors
- prunes common non-workspace directories such as `target`, `node_modules`,
  `dist`, `build`, and `.git` before recursing
- caches parsed frontmatter across the recursive traversal so each file is
  read and parsed at most once (eliminates the previous double-parse of
  `is_index_file` + title extraction, and shares results with
  `exclude_patterns_for_dir` and `find_any_index_in_dir`)
- logs how many directories were explored and pruned, with debug-level path
  lists for diagnosis
