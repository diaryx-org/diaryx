---
title: Storage
description: Storage abstraction layer
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/storage/index.ts)'
  - '[localWorkspaceRegistry.svelte.ts](/apps/web/src/lib/storage/localWorkspaceRegistry.svelte.ts)'
  - '[sqliteStorage.ts](/apps/web/src/lib/storage/sqliteStorage.ts)'
  - '[sqliteStorageBridge.js](/apps/web/src/lib/storage/sqliteStorageBridge.js)'
exclude:
  - '*.lock'
---

# Storage

Storage utilities for workspace registry and persistence.

## Files

| File | Purpose |
|------|---------|
| `localWorkspaceRegistry.svelte.ts` | Local workspace registry + current workspace selection state (reactive, mirrored to localStorage). |
| `sqliteStorage.ts` | SQLite-based CRDT storage |
| `sqliteStorageBridge.js` | Bridge to sql.js |
