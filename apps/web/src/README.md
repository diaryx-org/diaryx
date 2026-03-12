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

For Playwright sync coverage, `App.svelte` also exposes a localhost-only
`globalThis.__diaryx_e2e` bridge in dev runs. That bridge is intentionally
limited to test helpers such as root-path lookup, content mutation, sync-status
inspection, and provider-link introspection so browser E2E flows can drive the
generic host without importing duplicate app modules through Vite.

## Rename Behavior

`App.svelte` treats title-driven renames as a path transition:

- Saves unsaved body edits before rename
- Updates `entryStore` with the new path/frontmatter
- Remaps active collaboration path tracking to the renamed file

## Starter Workspace Bootstrap

`App.svelte` bootstraps starter content for first-run users.
On iOS Tauri, first-run shows the welcome/onboarding screen before any
workspace creation.
On iOS Tauri, if the selected workspace directory exists but has no root index
and no files, startup seeds the same starter workspace content used by the web
first-run flow.
If a workspace root already exists (for example, pre-initialized by the Tauri
backend), the "Get Started" flow upgrades that default scaffold to starter
content instead of opening the add-workspace wizard.
During onboarding bundle install, starter workspace frontmatter under
`plugins.<plugin-id>.permissions` is treated as authoritative and can suppress
the browser install review dialog before plugin bytes are installed.

## Mobile Swipe Behavior

`App.svelte` supports touch swipe gestures:

- Swipe down from the top edge opens the command palette.
- Swipe right closes an open right sidebar first; if right is already closed, it opens the left sidebar only when the gesture starts from the left screen edge.
- Swipe left closes an open left sidebar first; if left is already closed, it opens the right sidebar only when the gesture starts from the right screen edge.
- Gestures that begin inside modal/dialog surfaces or turn into an active text selection are ignored so marketplace/settings navigation and editor selection do not accidentally trigger sidebars.
