---
title: Storage
description: Storage abstraction layer
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/storage/index.ts)'
  - '[localWorkspaceRegistry.svelte.ts](/apps/web/src/lib/storage/localWorkspaceRegistry.svelte.ts)'
exclude:
  - '*.lock'
---

# Storage

Storage utilities for workspace registry and persistence.

## Files

| File | Purpose |
|------|---------|
| `localWorkspaceRegistry.svelte.ts` | Local workspace registry + current workspace selection state (reactive, mirrored to localStorage). |
