---
title: Storage
description: Storage abstraction layer
part_of: '[README](/apps/web/src/lib/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# Storage

Storage utilities for workspace registry and persistence.

## Files

| File | Purpose |
|------|---------|
| `localWorkspaceRegistry.svelte.ts` | Local workspace registry + current workspace selection state (reactive, mirrored to localStorage). Includes plugin storage helpers and normalized workspace-provider link helpers (`getWorkspaceProviderLinks`, `getPrimaryWorkspaceProviderLink`). |
| `pluginFileSystem.ts` | Creates `JsAsyncFileSystem` callbacks that dispatch to a storage Extism plugin (S3, Google Drive). Used by `WorkerBackendNew.initPluginStorage()`. |
