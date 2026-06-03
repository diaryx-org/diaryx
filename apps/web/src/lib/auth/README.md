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
| `authService.ts`      | Legacy class-based HTTP client for endpoints that have NOT been migrated to `coreAuthService` yet — passkeys, Stripe/Apple billing, managed AI/proxy helpers, namespaces |
| `authStore.svelte.ts` | Svelte 5 reactive auth state. Routes the 10 core endpoints through `coreAuthService` and keeps the legacy `authService` wired for everything else |
| `coreAuthTypes.ts`    | Narrow `CoreAuthService` interface mirroring `diaryx_core::auth::AuthService`'s public methods |
| `coreAuthRouter.ts`   | Runtime router: picks `wasmAuthService` in the browser and `tauriAuthService` on Tauri |
| `wasmAuthService.ts`  | Browser impl — routes every `AuthClient` call through the backend worker (see `lib/backend/wasmWorkerNew.ts`) so WASM is only instantiated once; HTTP + localStorage callbacks are passed in via `Comlink.proxy` and run on the main thread |
| `tauriAuthService.ts` | Tauri impl — thin typed wrappers around the `auth_*` IPC commands in `apps/tauri/src-tauri/src/auth_commands.rs` |

## coreAuthService

`coreAuthService` in `coreAuthRouter.ts` is the shared entry point for the
10 methods that the Rust `AuthService<C>` implements:

```
requestMagicLink / verifyMagicLink / verifyCode
getMe / refreshToken / logout
getDevices / renameDevice / deleteDevice
deleteAccount
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

Device management failures also preserve server-provided JSON `error` /
`message` fields in `AuthError.message`. Account settings can therefore show
specific delete/rename causes such as "device not found" or the server's
current-device rejection instead of a generic failure string.

Workspace namespace filtering now treats `metadata.type === "workspace"` as the
canonical marker and still accepts legacy `metadata.kind === "workspace"` data
when `type` is absent.

`authStore.svelte.ts` keeps a nullable storage-usage slot for compatibility and
also exposes helpers:

- `getStorageUsage()`
- `refreshUserStorageUsage()`

`refreshUserStorageUsage()` is currently a no-op because the old
`/api/user/storage` endpoint is no longer implemented server-side.

Provider sync/share flows read auth state through the generic plugin runtime
context. `server_url` and `auth_token` stay runtime-scoped; the host no longer
mirrors them into provider plugin config or command params.
