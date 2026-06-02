---
title: Storage
description: Storage abstraction layer
part_of: '[README](/apps/web/src/lib/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# Storage

Storage utilities for local workspace registry and persistence.

Workspace identity is local. A registered workspace points to a local backend
storage choice and, on Tauri/FSA paths, a user-selected folder. Provider-link
metadata is still parsed for backward compatibility with older sync-enabled
registries, but the active UI no longer uses it to offer remote workspace
sync/restore flows.

## Files

| File | Purpose |
|------|---------|
| `localWorkspaceRegistry.svelte.ts` | Local workspace registry + current workspace selection state (reactive, mirrored to localStorage). Includes legacy plugin-storage and workspace-provider link helpers for migration compatibility. |
| `pluginFileSystem.ts` | Legacy plugin-storage filesystem callbacks. Not exposed as a sync/storage choice in the active settings UI. |
