---
title: Lib Stores
description: Svelte stores for UI preferences
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[linkFormatStore.svelte.ts](/apps/web/src/lib/stores/linkFormatStore.svelte.ts)'
  - '[workspaceConfigStore.svelte.ts](/apps/web/src/lib/stores/workspaceConfigStore.svelte.ts)'
  - '[theme.svelte.ts](/apps/web/src/lib/stores/theme.svelte.ts)'
  - '[templateContextStore.svelte.ts](/apps/web/src/lib/stores/templateContextStore.svelte.ts)'
  - '[appearance.svelte.ts](/apps/web/src/lib/stores/appearance.svelte.ts)'
  - '[appearance.types.ts](/apps/web/src/lib/stores/appearance.types.ts)'
  - '[appearance.utils.ts](/apps/web/src/lib/stores/appearance.utils.ts)'
  - '[appearance.presets.ts](/apps/web/src/lib/stores/appearance.presets.ts)'
exclude:
  - '*.lock'
---

# Lib Stores

Svelte stores for UI preferences and settings.

## Files

| File | Purpose |
|------|---------|
| `linkFormatStore.svelte.ts` | Link format settings (persisted to workspace config) |
| `workspaceConfigStore.svelte.ts` | Workspace config store (persisted to root index frontmatter) |
| `theme.svelte.ts` | Theme (light/dark) store |
| `templateContextStore.svelte.ts` | Template context for live variable resolution in editor |
| `appearance.svelte.ts` | Appearance store (theme library + active preset, accent hue, typography, layout) |
| `appearance.types.ts` | Type definitions for the appearance system |
| `appearance.utils.ts` | OKLch parsing, accent hue shifting, CSS variable helpers |
| `appearance.presets.ts` | Built-in theme presets (default, sepia, nord, rosé pine) |

## Appearance Store

`appearance.svelte.ts` manages built-in + installed theme library entries, built-in + installed typography preset entries, active theme/typography selection, accent color overrides, and per-field typography overrides (font family, size, line height, content width). It is orthogonal to `theme.svelte.ts` (light/dark/system mode) — a user picks a theme preset + typography preset + mode independently.

Themes define full OKLch color palettes for both light and dark modes. The accent hue override shifts primary/accent colors while preserving lightness and chroma. Typography presets define base editor typography/layout values, while per-field overrides remain adjustable and are stored separately. Effective typography values control CSS custom properties (`--editor-font-family`, `--editor-font-size`, `--editor-line-height`, `--editor-content-max-width`).

Persisted to `localStorage["diaryx-appearance"]` (active selections + typography overrides), `localStorage["diaryx-theme-library-v1"]` (installed non-built-in themes), and `localStorage["diaryx-typography-library-v1"]` (installed non-built-in typography presets). Migrates from the legacy `readableLineLength` boolean and legacy inline typography/layout fields on first load. A FOUC-prevention script in `index.html` reads cached CSS vars synchronously before first paint.

## Workspace Config Store

`workspaceConfigStore.svelte.ts` provides reactive access to the full `WorkspaceConfig` stored in the workspace root index frontmatter. Used by `WorkspaceSettings.svelte` to manage settings like `auto_update_timestamp`, `sync_title_to_heading`, `filename_style`, `default_template`, and `default_audience`.

Settings that were previously in browser localStorage (`default_template`) are migrated to workspace config on first load so they sync across devices.
