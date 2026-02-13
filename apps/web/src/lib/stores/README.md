---
title: Lib Stores
description: Svelte stores for UI preferences
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[formattingStore.svelte.ts](/apps/web/src/lib/stores/formattingStore.svelte.ts)'
  - '[linkFormatStore.svelte.ts](/apps/web/src/lib/stores/linkFormatStore.svelte.ts)'
  - '[workspaceConfigStore.svelte.ts](/apps/web/src/lib/stores/workspaceConfigStore.svelte.ts)'
  - '[theme.svelte.ts](/apps/web/src/lib/stores/theme.svelte.ts)'
exclude:
  - '*.lock'
---

# Lib Stores

Svelte stores for UI preferences and settings.

## Files

| File | Purpose |
|------|---------|
| `formattingStore.svelte.ts` | Text formatting preferences |
| `linkFormatStore.svelte.ts` | Link format settings (persisted to workspace config) |
| `workspaceConfigStore.svelte.ts` | Workspace config store (persisted to root index frontmatter) |
| `theme.svelte.ts` | Theme (light/dark) store |

## Workspace Config Store

`workspaceConfigStore.svelte.ts` provides reactive access to the full `WorkspaceConfig` stored in the workspace root index frontmatter. Used by `WorkspaceSettings.svelte` and `TemplateSettings.svelte` to manage settings like `auto_update_timestamp`, `sync_title_to_heading`, `filename_style`, `daily_entry_folder`, `default_template`, and `daily_template`.

Settings that were previously in browser localStorage (`daily_entry_folder`, `default_template`, `daily_template`) are migrated to workspace config on first load so they sync across devices.
