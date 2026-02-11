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
| `authService.ts`      | Magic link auth API, snapshot upload/download, storage usage API, attachment multipart transfer API |
| `authStore.svelte.ts` | Authentication state store + synced attachment usage state |

Snapshot helpers support `include_attachments=true|false` (default `true`) for
both upload and download bootstrap flows.

`authService.ts` also provides incremental attachment transfer calls:

- `initAttachmentUpload(...)`
- `uploadAttachmentPart(...)`
- `completeAttachmentUpload(...)`
- `downloadAttachment(...)`

Quota rejections (`413` + `storage_limit_exceeded`) are parsed into
`AuthError` messages with usage/limit context for UI and queue handling.

`authStore.svelte.ts` also exposes storage usage helpers:

- `getStorageUsage()`
- `refreshUserStorageUsage()`
