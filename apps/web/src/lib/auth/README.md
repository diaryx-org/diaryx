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
| `authService.ts`      | Legacy class-based HTTP client for endpoints that have NOT been migrated to `coreAuthService` yet — passkeys, Stripe/Apple billing, snapshots, attachments, namespaces |
| `authStore.svelte.ts` | Svelte 5 reactive auth state. Routes the 12 core endpoints through `coreAuthService` and keeps the legacy `authService` wired for everything else |
| `coreAuthTypes.ts`    | Narrow `CoreAuthService` interface mirroring `diaryx_core::auth::AuthService`'s 12 public methods |
| `coreAuthRouter.ts`   | Runtime router: picks `wasmAuthService` in the browser and `tauriAuthService` on Tauri |
| `wasmAuthService.ts`  | Browser impl — routes every `AuthClient` call through the backend worker (see `lib/backend/wasmWorkerNew.ts`) so WASM is only instantiated once; HTTP + localStorage callbacks are passed in via `Comlink.proxy` and run on the main thread |
| `tauriAuthService.ts` | Tauri impl — thin typed wrappers around the `auth_*` IPC commands in `apps/tauri/src-tauri/src/auth_commands.rs` |

## coreAuthService

`coreAuthService` in `coreAuthRouter.ts` is the shared entry point for the
12 methods that the Rust `AuthService<C>` implements:

```
requestMagicLink / verifyMagicLink / verifyCode
getMe / refreshToken / logout
getDevices / renameDevice / deleteDevice
deleteAccount
createWorkspace / renameWorkspace / deleteWorkspace
```

Both impls implement `CoreAuthService` from `coreAuthTypes.ts`. On the
browser, the raw session token lives in an HttpOnly cookie and the wasm
layer only knows "session exists" / "session cleared". On Tauri, the
token lives in the OS keyring (`org.diaryx.app`/`session_token`) and is
owned by the `KeyringAuthenticatedClient` running in the Rust host — the
verify commands mirror it into the legacy keychain slot for now so that
passkey/billing endpoints still riding `proxyFetch` keep working. That
mirror is the single remaining place JS can read the token on Tauri; it
will be removed once those endpoints move onto `coreAuthService` too.

The verify methods accept an optional `replaceDeviceId`, and `AuthError`
surfaces the server's `devices[]` list on 403 device-limit responses so
`DeviceReplacementDialog` can drive the retry loop.

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
