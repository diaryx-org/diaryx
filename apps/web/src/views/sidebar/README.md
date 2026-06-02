---
title: Sidebar Views
description: Sidebar components
part_of: '[README](/apps/web/src/views/README.md)'
exclude:
  - '*.lock'
---

# Sidebar Views

Sidebar and mobile navigation components.

## Files

| File | Purpose |
|------|---------|
| `MobileActionSheet.svelte` | Mobile action sheet for sidebar actions, including the desktop-only file-manager reveal action when Tauri exposes it |

## Workspace Switching

The workspace selector lives in `lib/WorkspaceSelector.svelte` and is hosted by
`LeftSidebar.svelte`. Its New workspace action delegates to `App.svelte`, which
opens the folder picker directly instead of reopening the welcome/onboarding
screen.

## File Navigation Fallback

`LeftSidebar.svelte` can be put into file-navigation mode by `App.svelte`.
That mode replaces the workspace tree with the currently authorized single
file and an iOS Files picker action. It is used only when the app has a
file-level grant instead of a folder-level workspace grant.
