---
title: web src
description: Source code for the Diaryx web application
part_of: '[README](/apps/web/README.md)'
contents:
  - '[README](/apps/web/src/controllers/README.md)'
  - '[README](/apps/web/src/lib/README.md)'
  - '[README](/apps/web/src/models/README.md)'
  - '[README](/apps/web/src/views/README.md)'
  - '[LICENSE](/apps/web/src/LICENSE.md)'
attachments:
  - '[App.svelte](/apps/web/src/App.svelte)'
  - '[main.ts](/apps/web/src/main.ts)'
  - '[app.css](/apps/web/src/app.css)'
exclude:
  - '*.lock'
  - 'test/**'
---

# Diaryx Web Source

This directory contains the source code for the Diaryx web application.

## Structure

| File/Directory | Purpose |
|----------------|---------|
| `App.svelte` | Main application component |
| `main.ts` | Application entry point |
| `app.css` | Global styles |
| `controllers/` | Controller logic for UI actions |
| `lib/` | Shared libraries and components |
| `models/` | Stores and services |
| `views/` | View components |

## Rename Behavior

`App.svelte` treats title-driven renames as a path transition:

- If the current entry has unsaved body edits, it saves before renaming.
- It updates `entryStore` (not local derived state) with the new path/frontmatter.
- It remaps collaboration tracking to the renamed path immediately, so follow-up
  body saves continue syncing under the new file name.
- For remote syncs where rename arrives as delete+create events, it applies a
  fallback remap for the currently open entry to keep the editor and metadata
  panel attached to the renamed file path.
- Rename fallback matching normalizes `part_of` values before comparison using
  `diaryx_core` link-parser-equivalent rules (markdown links, angle-bracket
  links, and relative path normalization), so remote rename remaps are not
  skipped due link-format differences.

## Sync Bootstrap Safeguards

CrdtFs (the filesystem decorator that auto-updates CRDTs on file writes) starts
**disabled** by default. It is only enabled after the sync handshake completes
(`onWorkspaceSynced`), or immediately in local-only mode. This prevents file
writes during import/bootstrap from creating local CRDT operations that would
merge with server state and cause content duplication.

## Share Sidebar

`lib/share/ShareTab.svelte` now hosts two share sub-tabs in the right sidebar:

- `Live Collaboration` for session hosting/joining.
- `Publishing` for site setup, publish-now, and token management against
  `/api/workspaces/{id}/site*` endpoints using the authenticated default workspace.
