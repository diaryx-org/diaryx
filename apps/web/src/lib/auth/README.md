---
title: Auth
description: Authentication services and stores
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[index.ts](/apps/web/src/lib/auth/index.ts)"
  - "[authService.ts](/apps/web/src/lib/auth/authService.ts)"
  - "[authStore.svelte.ts](/apps/web/src/lib/auth/authStore.svelte.ts)"
exclude:
  - "*.lock"
---

# Auth

Authentication services and stores for sync server login.

## Files

| File                  | Purpose                                                    |
| --------------------- | ---------------------------------------------------------- |
| `authService.ts`      | Magic link auth API, snapshot upload/download, storage usage API |
| `authStore.svelte.ts` | Authentication state store + synced attachment usage state |

Snapshot helpers support `include_attachments=true|false` (default `true`) for
both upload and download bootstrap flows.

`authStore.svelte.ts` also exposes storage usage helpers:

- `getStorageUsage()`
- `refreshUserStorageUsage()`
