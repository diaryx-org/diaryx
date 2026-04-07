---
title: Stores
description: Svelte stores for reactive state
part_of: '[README](/apps/web/src/models/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
  - '*.test.ts'
---

# Stores

Svelte stores for reactive application state.

## Files

| File | Purpose |
|------|---------|
| `collaborationStore.svelte.ts` | Sync/session status state (status/progress/error) used by generic plugin-driven UI surfaces. |
| `entryStore.svelte.ts` | Current entry + dirty/saving state, plus the shared 300ms auto-save debounce helper. |
| `pluginStore.svelte.ts` | Plugin manifest aggregation and derived UI contribution selectors (settings/sidebar/toolbar/status/commands/providers), including compatibility selectors for legacy provider/block-picker surfaces and `InlineMark` editor insert commands. |
| `permissionStore.svelte.ts` | Runtime plugin permission checks, pending request queue, session cache, and root-frontmatter persistence hooks. Plugin storage remains sandboxed per plugin and is treated as allowed-by-default to match the native host. |
| `uiStore.svelte.ts` | Dialog/sidebar state and UI toggles. |
| `workspaceStore.svelte.ts` | Workspace tree state, expanded-node persistence, and guest-session tree preservation helpers. |

`pluginStore` is now the primary source for sync/share/history/status surfaces;
the host renders these generically from plugin contributions.

Workspace switches re-run `pluginStore.init(...)` against the new backend so
workspace-scoped plugin manifests update immediately without requiring a full
webview reload.
