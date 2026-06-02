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

`WelcomeScreen.svelte` owns first-run workspace setup and restore. Its primary
path is folder-first: users create a new Diaryx workspace in a selected folder
or open an existing workspace folder before choosing any sync/provider option.

- Bundle selection is now a starter-content choice for the selected folder
  workspace rather than a sync-provider choice.
- iOS Tauri can expose an optional single-file fallback when cloud providers
  allow opening a Markdown file but gray out folder selection. That path opens
  the file directly and does not build a full workspace tree.
- Built-in host providers such as Apple/Tauri iCloud Drive still appear in the
  provider-choice UI for moving the current workspace, but not as the default
  first-run path.
- The workspace picker can restore either server-backed namespaces or provider-owned restore targets surfaced directly by the host.
- Provider-backed namespaces that exist on the account but are unsupported on the current client stay visible in the picker with an unavailable message instead of disappearing from restore UI.
