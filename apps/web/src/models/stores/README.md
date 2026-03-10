---
title: Stores
description: Svelte stores for reactive state
part_of: '[README](/apps/web/src/models/README.md)'
attachments:
  - '[index.ts](/apps/web/src/models/stores/index.ts)'
  - '[collaborationStore.svelte.ts](/apps/web/src/models/stores/collaborationStore.svelte.ts)'
  - '[entryStore.svelte.ts](/apps/web/src/models/stores/entryStore.svelte.ts)'
  - '[sitePublishingStore.svelte.ts](/apps/web/src/models/stores/sitePublishingStore.svelte.ts)'
  - '[pluginStore.svelte.ts](/apps/web/src/models/stores/pluginStore.svelte.ts)'
  - '[permissionStore.svelte.ts](/apps/web/src/models/stores/permissionStore.svelte.ts)'
  - '[uiStore.svelte.ts](/apps/web/src/models/stores/uiStore.svelte.ts)'
  - '[workspaceStore.svelte.ts](/apps/web/src/models/stores/workspaceStore.svelte.ts)'
exclude:
  - '*.lock'
  - '*.test.ts'
---

# Stores

Svelte stores for reactive application state.

## Files

| File | Purpose |
|------|---------|
| `collaborationStore.svelte.ts` | Sync/session status state (status/progress/error) used by generic plugin-driven UI surfaces. |
| `entryStore.svelte.ts` | Current entry + dirty/saving state. |
| `sitePublishingStore.svelte.ts` | Publishing state for site config, publish actions, and token lifecycle. |
| `pluginStore.svelte.ts` | Plugin manifest aggregation and derived UI contribution selectors (settings/sidebar/toolbar/status/commands/providers), including compatibility selectors for legacy provider/block-picker surfaces and `InlineMark` editor insert commands. |
| `permissionStore.svelte.ts` | Runtime plugin permission checks, pending request queue, session cache, and root-frontmatter persistence hooks. |
| `uiStore.svelte.ts` | Dialog/sidebar state and UI toggles. |
| `workspaceStore.svelte.ts` | Workspace tree state, expanded-node persistence, and guest-session tree preservation helpers. |

`pluginStore` is now the primary source for sync/share/history/status surfaces;
the host renders these generically from plugin contributions.
