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

`WelcomeScreen.svelte` owns first-run workspace setup and restore.

- Bundle selection feeds the provider-choice step when a starter bundle includes workspace providers.
- Built-in host providers such as Apple/Tauri iCloud Drive also appear in that same provider-choice UI instead of as separate top-level onboarding actions.
- The workspace picker can restore either server-backed namespaces or provider-owned restore targets surfaced directly by the host.
- Provider-backed namespaces that exist on the account but are unsupported on the current client stay visible in the picker with an unavailable message instead of disappearing from restore UI.
