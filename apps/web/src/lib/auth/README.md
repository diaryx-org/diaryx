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

Init and complete upload requests include both `entry_path` and
`attachment_path` so the server can canonicalize attachment refs consistently
before storing and validating resumable upload sessions.

Quota rejections (`413` + `storage_limit_exceeded`) are parsed into
`AuthError` messages with usage/limit context for UI and queue handling.

`authStore.svelte.ts` keeps synced storage usage in the main auth state and also exposes helpers:

- `getStorageUsage()`
- `refreshUserStorageUsage()`

`getUserStorageUsage()` sends `cache: "no-store"` plus `Cache-Control: no-cache`
and `Pragma: no-cache` headers so `/api/user/storage` refreshes always request
fresh usage data.
