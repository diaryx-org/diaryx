---
title: Stores
description: Svelte stores for reactive state
part_of: '[README](/apps/web/src/models/README.md)'
attachments:
  - '[index.ts](/apps/web/src/models/stores/index.ts)'
  - '[collaborationStore.svelte.ts](/apps/web/src/models/stores/collaborationStore.svelte.ts)'
  - '[entryStore.svelte.ts](/apps/web/src/models/stores/entryStore.svelte.ts)'
  - '[sitePublishingStore.svelte.ts](/apps/web/src/models/stores/sitePublishingStore.svelte.ts)'
  - '[shareSessionStore.svelte.ts](/apps/web/src/models/stores/shareSessionStore.svelte.ts)'
  - '[pluginStore.svelte.ts](/apps/web/src/models/stores/pluginStore.svelte.ts)'
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
| `collaborationStore.svelte.ts` | Real-time collaboration state |
| `entryStore.svelte.ts` | Current entry state |
| `sitePublishingStore.svelte.ts` | Publishing state for default workspace site configuration, publish actions, and token lifecycle |
| `shareSessionStore.svelte.ts` | Share session state |
| `pluginStore.svelte.ts` | Plugin manifest aggregation + derived UI contribution selectors (settings/sidebar/toolbar/status/commands). Runtime manifests can override backend manifests by plugin id (used by browser-loaded sync plugin). |
| `uiStore.svelte.ts` | UI state (sidebars, dialogs) |
| `workspaceStore.svelte.ts` | Workspace tree state. Lazy subtree updates short-circuit as soon as the target node is found, avoiding full-tree traversal on folder expansion in large workspaces. |

`collaborationStore.svelte.ts` normalizes unknown/object-shaped sync errors
into readable strings (including nested `message`/`error` fields and JSON
payloads) so UI surfaces like `SyncStatusIndicator` do not display raw
`[object Object]`.
