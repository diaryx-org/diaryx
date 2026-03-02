---
title: web src
description: Source code for the Diaryx web application
part_of: "[README](/apps/web/README.md)"
contents:
  - "[README](/apps/web/src/controllers/README.md)"
  - "[README](/apps/web/src/lib/README.md)"
  - "[README](/apps/web/src/models/README.md)"
  - "[README](/apps/web/src/views/README.md)"
  - "[LICENSE](/apps/web/src/LICENSE.md)"
attachments:
  - "[App.svelte](/apps/web/src/App.svelte)"
  - "[main.ts](/apps/web/src/main.ts)"
  - "[app.css](/apps/web/src/app.css)"
exclude:
  - "*.lock"
  - "test/**"
---

# Diaryx Web Source

Source tree for the Diaryx web frontend.

## Structure

| File/Directory | Purpose |
| --- | --- |
| `App.svelte` | Main application shell |
| `main.ts` | Application entrypoint |
| `app.css` | Global styles |
| `controllers/` | UI action controllers |
| `lib/` | Shared libraries and components |
| `models/` | Stores and services |
| `views/` | View components |

## Sync/Share Architecture

Web no longer owns a CRDT bridge layer. Sync/share/provider/history behavior is
implemented by the sync plugin and consumed through generic plugin command/UI
infrastructure in the host.

`App.svelte` listens to backend filesystem events and refreshes workspace tree +
active entry state from those events, rather than wiring direct host CRDT
bridge callbacks.

## Rename Behavior

`App.svelte` treats title-driven renames as a path transition:

- Saves unsaved body edits before rename
- Updates `entryStore` with the new path/frontmatter
- Remaps active collaboration path tracking to the renamed file
