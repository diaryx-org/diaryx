---
title: Auth
description: Authentication services and stores
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
  - "**/*.ts"
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

Workspace namespace filtering now treats `metadata.type === "workspace"` as the
canonical marker and still accepts legacy `metadata.kind === "workspace"` data
when `type` is absent.

For snapshot uploads, web uses XHR when byte-progress callbacks are requested.
Tauri uses `proxyFetch` (native HTTP) instead of XHR to avoid WebView/CORS edge
cases; upload calls still succeed without byte-level XHR progress events.

Upload response parsing now tolerates empty successful (`2xx`) bodies by
defaulting `files_imported` to `0`, and rate-limit responses (`429`) include
`Retry-After` context in surfaced error messages.

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

Provider sync/share flows read auth state through the generic plugin runtime
context. `server_url` and `auth_token` stay runtime-scoped; the host no longer
mirrors them into provider plugin config or command params.
