---
title: Views
description: View components
part_of: '[README](/apps/web/src/README.md)'
contents:
  - '[README](/apps/web/src/views/editor/README.md)'
  - '[README](/apps/web/src/views/layout/README.md)'
  - '[README](/apps/web/src/views/marketplace/README.md)'
  - '[README](/apps/web/src/views/shared/README.md)'
  - '[README](/apps/web/src/views/sidebar/README.md)'
exclude:
  - '*.lock'
---

# Views

View components organized by feature area.

## Structure

| Directory | Purpose |
|-----------|---------|
| `editor/` | Editor-related views |
| `layout/` | Layout components |
| `marketplace/` | Marketplace panels and plugin/theme browsing views |
| `shared/` | Shared view components |
| `sidebar/` | Sidebar components |

## Onboarding

`WelcomeScreen.svelte` owns first-run workspace setup. The active path is
folder-first: users create a new Diaryx workspace in a selected folder or open
an existing workspace folder.

- Bundle selection is now a starter-content choice for the selected folder
  workspace.
- iOS Tauri can expose an optional single-file fallback when cloud providers
  allow opening a Markdown file but gray out folder selection. That path opens
  the file directly and does not build a full workspace tree.
- Signing in is account-oriented and no longer opens remote workspace restore
  or provider-backed workspace creation flows.
- Users who want cross-device access should place the selected workspace folder
  in iCloud Drive, Dropbox, Syncthing, Git, or another external sync tool.
