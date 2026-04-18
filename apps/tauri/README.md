---
title: tauri
description: Web app + native backend
author: adammharris
part_of: '[README](/apps/README.md)'
updated: 2026-03-29T19:40:38-06:00
exclude:
- '**/gen'
---
# Diaryx Tauri

The Tauri backend for Diaryx, providing native filesystem access for the web frontend.

The desktop/mobile config also enables Tauri's built-in `asset` protocol for common local-user directories so image previews can use native file-backed URLs (`convertFileSrc`) instead of copying attachment bytes through JS when the workspace path falls inside those scopes. Preview flows still fall back to the blob/binary path when a file is outside scope or native loading is unavailable. The Rust app enables the matching Tauri `protocol-asset` feature in`src-tauri/Cargo.toml`.

Mac App Store / TestFlight builds now keep sandbox-safe behavior for workspace folders by persisting security-scoped bookmarks for any folder chosen through the native picker. On the next launch or workspace switch, the app resolves the bookmark and re-opens access before reading the workspace-local `.diaryx` metadata directory, which keeps plugin installation working for externally chosen folders in sandboxed releases. The App Store entitlements therefore need
both `com.apple.security.files.user-selected.read-write` and
`com.apple.security.files.bookmarks.app-scope` so the sandboxed build can
persist and reopen those bookmarks across launches.

Shared web-side folder picks now bridge back into Rust through
`authorize_workspace_path` as well. That command lets flows like "open existing
folder" and "relocate workspace" turn a JS-selected path into a persisted
security-scoped bookmark immediately, instead of relying only on
`pick_workspace_folder`.

The Tauri backend also writes file-backed logs under the app data directory
(`logs/diaryx.log`). The shared Debug Info panel exposes the resolved log file
path, can read the current log contents directly in-app, and can reveal the
file in Finder on desktop builds, which makes TestFlight/App Store debugging
much easier than relying on transient console output.

For plugin parity with the browser host, Tauri now also exposes native plugin
inspection before install (so the shared frontend can review requested
permissions on both paths) and temporary file-byte bridging for
`host_request_file` during plugin command execution.

Desktop builds also register `tauri-plugin-opener` so the shared left sidebar
can reveal the selected entry in Finder/Explorer/the system file manager.
Tauri's reveal API is desktop-only, so the menu item stays hidden on iOS and
Android.

On macOS desktop, the shared frontend shell reserves a top drag strip via
`--titlebar-area-height` and starts dragging through Tauri's window API from
that strip. The desktop capability file grants
`core:window:allow-start-dragging` so the overlay titlebar remains draggable
even on the welcome screen and in narrow windows. Shared chrome surfaces route
through a frontend helper that skips interactive controls so buttons remain
clickable while empty shell areas still drag the window.

## Architecture

The Tauri app shares the same Svelte frontend as the web app (`apps/web`), but instead of using WebAssembly with an in-memory filesystem, it uses Tauri IPC to communicate with a Rust backend that accesses the real filesystem.

```
┌─────────────────────────────────────────┐
│           Svelte Frontend               │
│         (apps/web/src/lib)              │
└─────────────────┬───────────────────────┘
                  │
        ┌─────────┴─────────┐
        │                   │
        ▼                   ▼
┌───────────────┐   ┌───────────────┐
│  WasmBackend  │   │ TauriBackend  │
│ (IndexedDB +  │   │ (Tauri IPC)   │
│  InMemoryFS)  │   │               │
└───────────────┘   └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ commands.rs   │
                    │ (diaryx_core) │
                    └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ RealFileSystem│
                    └───────────────┘
```

## Building

```bash
# Development
cd apps/tauri
bun install
bun run tauri dev

# Production build
bun run tauri build
```

### macOS: suppressing keychain prompts on every `tauri dev` rebuild

The Tauri app stores auth credentials in the macOS keychain (see
`src-tauri/src/credentials.rs` and `auth_client.rs`). macOS binds keychain
ACLs to the binary's code signature. Ad-hoc-signed cargo debug builds (the
default on Apple Silicon) use an untrusted signature, so keychain "Always
Allow" grants don't persist across rebuilds — each rebuild re-prompts.

The fix is to sign debug builds with an Apple-anchored identity (the same
thing Xcode does by default). The free "Apple Development" identity works.

```bash
# One-time: validates your identity + authorizes codesign to use its key.
./scripts/setup-macos-dev-signing.sh

# Enable signing for dev builds
export DIARYX_DEV_SIGN=1
bun tauri dev
```

On the first keychain prompt after signing, click "Always Allow" once.
Subsequent rebuilds re-sign with the same identity, so the grant persists.

By default the scripts auto-detect the first `Apple Development: ...` identity
in your login keychain. To use a different one, set
`DIARYX_DEV_SIGN_IDENTITY` to the exact identity name before running setup
and/or `tauri dev`. Unset `DIARYX_DEV_SIGN` to fall back to normal ad-hoc
signing.

If you don't have an Apple Development identity yet, get one via Xcode →
Settings → Accounts → your Apple ID → Manage Certificates → + "Apple
Development" (free with any Apple ID).

Tauri dev now ignores markdown edits under repo code directories
(`apps/**`, `crates/**`, `workers/**`, `scripts/**`) via the workspace
`.taurignore`, and the shared Vite dev server also ignores `**/*.md`.
That keeps in-app editing of workspace documentation from triggering a frontend
reload while `cargo tauri dev` is running against this repo as a workspace.

Shared Tauri frontend startup now also treats the initial IPC bridge as
best-effort during dev reloads: `initialize_app` / `reinitialize_workspace`
have a frontend timeout plus a short dev-only retry window so transient
custom-protocol fallback issues can recover after the webview remounts.

For iOS dev builds, `bun run tauri:ios` now runs `bun run clean:ios-swift-cache`
first. This clears stale `swift-rs` module artifacts for Tauri and Tauri plugin
build scripts under `target/` that can break builds after moving the repository
to a new absolute path.

## Debug IPC Listener

For automated testing and scripted control of a running dev build (e.g.
driving the app from CI, shell scripts, or an AI assistant), the Tauri
backend exposes a minimal HTTP IPC surface on `127.0.0.1` behind three
layers of gating:

1. Compiled out entirely in release builds (`#[cfg(debug_assertions)]`
   in `src-tauri/src/dev_ipc.rs` and `lib.rs`).
2. Only starts when the `DIARYX_DEV_IPC=1` environment variable is set.
3. Every endpoint except `GET /health` requires an
   `X-Diaryx-Dev-Token` header matching a per-run random token written
   to the discovery file.

On startup the listener writes discovery JSON to
`apps/tauri/.dev-ipc.json` (build-time manifest-relative path) and to
`<app_data_dir>/.dev-ipc.json` as a fallback. Both files are deleted
when the guard drops on graceful shutdown. The discovery file is also
gitignored.

```bash
# Start dev build with the IPC listener enabled
cd apps/tauri && bun run tauri:dev-ipc

# From another terminal
bash ./scripts/dev-ipc.sh GET /health
bash ./scripts/dev-ipc.sh GET /state
bash ./scripts/dev-ipc.sh GET "/log?tail=100"
bash ./scripts/dev-ipc.sh POST /execute --data '{"type":"GetEntry","params":{"path":"README.md"}}'
bash ./scripts/dev-ipc.sh POST /emit    --data '{"event": "my-event", "payload": {}}'
bash ./scripts/dev-ipc.sh GET  /screenshot > /tmp/diaryx.png
```

### Endpoints

| Method | Path        | Description                                                                |
| ------ | ----------- | -------------------------------------------------------------------------- |
| GET    | `/health`   | Returns `{ok, version, pid}`. No auth required.                            |
| GET    | `/state`    | Workspace path, guest mode flag, app data dir, pid, version.               |
| GET    | `/log`      | Log file contents. Query: `?tail=N` (last N lines), `?previous=1` (rotated). |
| POST   | `/execute`  | Body is a `Command` JSON (`{"type":"X","params":{...}}`). Runs through the unified `commands::execute` pipeline. |
| POST   | `/emit`     | Emits a `tauri::Emitter` event into the frontend.                          |
| POST   | `/eval`     | Runs a JS string in the main webview. Extra-gated by `DIARYX_DEV_IPC_EVAL=1`. |
| GET    | `/screenshot` | Captures the app's native window via `xcap`. Default returns `image/png` bytes. `?format=json` returns base64. `?pid=<n>` overrides PID filter. On first use, macOS prompts for Screen Recording permission. |

The `/eval` escape hatch is intentionally separate: it can execute
arbitrary JS in the webview, so it's off by default even when the IPC
listener is running. Enable it explicitly when needed:

```bash
DIARYX_DEV_IPC=1 DIARYX_DEV_IPC_EVAL=1 bun run tauri dev
```

## Tauri Commands

All IPC commands are defined in `src-tauri/src/commands.rs` and registered in `src-tauri/src/lib.rs`.

### Auth Commands

Authentication runs through a native `AuthService<KeyringAuthenticatedClient>`
wired up in `src-tauri/src/auth_client.rs` + `auth_commands.rs`. The session
token lives in the OS keyring (service `org.diaryx.app`, account
`session_token`) and HTTP calls go through `reqwest` in Rust, so the raw
token never leaves the host process when using the 12 migrated endpoints.
Non-secret metadata (server URL, last email, workspace id) is persisted to
`<app_data>/auth.json`.

| Command                   | Description                                   |
| ------------------------- | --------------------------------------------- |
| `auth_server_url`         | Read currently configured sync server URL     |
| `auth_set_server_url`     | Rebuild the host client against a new URL     |
| `auth_is_authenticated`   | Keyring-presence check                        |
| `auth_get_metadata`       | Non-secret `{email, workspace_id}`            |
| `auth_request_magic_link` | POST `/auth/magic-link`                       |
| `auth_verify_magic_link`  | GET `/auth/verify` (+ `replace_device_id`)    |
| `auth_verify_code`        | POST `/auth/verify-code`                      |
| `auth_get_me`             | GET `/auth/me` (tier, devices, limits)        |
| `auth_refresh_token`      | Alias for `auth_get_me`                       |
| `auth_logout`             | POST `/auth/logout` + clear keyring           |
| `auth_get_devices`        | GET `/auth/devices`                           |
| `auth_rename_device`      | PATCH `/auth/devices/:id`                     |
| `auth_delete_device`      | DELETE `/auth/devices/:id`                    |
| `auth_delete_account`     | DELETE `/auth/account`                        |
| `auth_create_workspace`   | POST `/api/workspaces`                        |
| `auth_rename_workspace`   | PATCH `/api/workspaces/:id`                   |
| `auth_delete_workspace`   | DELETE `/api/workspaces/:id`                  |

Passkeys, Stripe/Apple billing, snapshots, attachments, and namespace
queries still ride the legacy `proxyFetch` path with a bearer token
mirrored into the `credentials.rs` keychain slot. That mirror goes away
once those endpoints migrate onto `AuthService` too.

## iOS Editor Toolbar

The `tauri-plugin-editor-toolbar` crate provides the native iOS keyboard
toolbar for the shared TipTap editor. It exposes block formatting, history,
inline formatting, links, plugin-contributed toolbar commands, and audience
visibility from Swift while delegating editor mutations through the shared
`globalThis.__diaryx_nativeToolbar` bridge in `apps/web/src/lib/Editor.svelte`.
The audience button mirrors the web `VisibilityPicker`: it loads available
audiences from the web backend, marks the current inline or block visibility
selection, can create a new audience tag, and clears visibility from the
current selection.

### Validation Commands

The validation system checks workspace link integrity and can automatically fix issues.


| Command                     | Description                                             |
| --------------------------- | ------------------------------------------------------- |
| `validate_workspace`        | Validate entire workspace from root                     |
| `validate_file`             | Validate a single file's links                          |
| `fix_validation_warning`    | Auto-fix any warning via the generic variant dispatcher |
| `fix_validation_error`      | Auto-fix any error via the generic variant dispatcher   |
| `fix_all_validation_issues` | Fix all errors and fixable warnings                     |


### Other Commands


| Category       | Commands                                                                                                                         |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Workspace      | `get_workspace_tree`, `get_filesystem_tree`, `create_workspace`, `reveal_in_file_manager`                                        |
| Entries        | `get_entry`, `save_entry`, `create_entry`, `delete_entry`, `move_entry`                                                          |
| Entries (cont) | `attach_entry_to_parent`, `convert_to_index`, `convert_to_leaf`                                                                  |
| Entries (cont) | `create_child_entry`, `rename_entry`, `ensure_daily_entry`                                                                       |
| Frontmatter    | `get_frontmatter`, `set_frontmatter_property`, `remove_frontmatter_property`                                                     |
| Attachments    | `get_attachments`, `upload_attachment`, `delete_attachment`                                                                      |
| Search         | `search_workspace`                                                                                                               |
| Export         | `get_available_audiences`, `plan_export`, `export_to_memory`, `export_to_html`                                                   |
| Backup         | `backup_workspace`, `restore_workspace`, `backup_to_s3`, `backup_to_google_drive`                                                |
| Import         | `import_from_zip`, `pick_and_import_zip`                                                                                         |
| Apple/iCloud   | `set_icloud_enabled`, `get_icloud_workspace_info`, `list_icloud_workspaces`, `link_icloud_workspace`, `restore_icloud_workspace` |


## Plugin Host Transport

The Tauri app is a generic Extism plugin host. It provides plugin runtime
context plus generic HTTP/WebSocket bridges, and plugins own sync/share
protocol details on top of those host capabilities. The Tauri backend should
not hardcode `SyncClient`-style transport logic for a specific provider.
Namespace server work is exposed through purpose-built `host_namespace_*`
functions, including namespace creation, object CRUD, and filtered object
metadata listing, so provider plugins do not need generic HTTP permission for
same-server namespace operations.

The shared frontend now uses the same requested-permission review flow for
local plugin installs on both browser and Tauri paths. Tauri provides
`inspect_user_plugin` for pre-install manifest inspection and
`execute_plugin_command_with_files` so plugin settings actions that rely on
`host_request_file` work the same way as browser-loaded plugins. Iframe-backed
plugin surfaces also use the guest `get_component_html` export directly via
`get_plugin_component_html`, falling back to `handle_command` only for older
plugins that have not added the dedicated export. Local `.wasm` installs now
also persist any manifest-declared default permissions immediately on install,
and runtime "Permission not configured" plugin errors are surfaced through the
shared permission banner flow instead of failing the command outright on the
first attempt. Native plugin install commands also log explicit stage failures
to the file-backed Tauri log (`inspect`, manifest load, workspace write, cache
reset), which makes iOS/TestFlight install issues debuggable from the shared
Debug Info panel. iOS builds also reduce Wasmtime's linear-memory reservation
before plugin instantiation so native inspection/install does not trip over the
default 4 GiB virtual-memory reservation inside the mobile sandbox.

## Desktop Updater

Direct-distribution desktop builds can include `tauri-plugin-updater` so the
installed app can download GitHub Release updates from inside Diaryx.

- **Feature flag**: `desktop-updater` for Windows, Linux, and non-App-Store macOS builds
- **Config source**: `apps/tauri/scripts/render-updater-config.mjs` writes the merged
`src-tauri/tauri.updater.conf.json` file from `TAURI_UPDATER_PUBLIC_KEY`
- **Release endpoint**: `https://github.com/diaryx-org/diaryx/releases/latest/download/latest.json`
- **Frontend behavior**: `App.svelte` kicks off a background update check through
`updaterService.ts` once the Tauri backend is ready and shows an install toast only when a new build is available

Mac App Store builds must not include the updater. That split now lives in the
Cargo features: direct desktop releases enable `desktop-updater`, while App
Store builds use `apple`.

## Apple IAP (In-App Purchases)

The Tauri app includes `tauri-plugin-iap` for StoreKit 2 integration on iOS and macOS (Mac App Store). This enables native subscription purchasing through the App Store.

- **Plugin**: `tauri-plugin-iap` v0.8 (StoreKit 2, iOS 15+/macOS 13+)
- **Product ID**: `diaryx_plus_monthly` (configured in App Store Connect)
- **Capabilities**: `iap:default` in both `mobile.json` and `default.json`
- **Feature flag**: `apple` — App Store build umbrella feature that currently enables `iap`, `icloud`, and excludes the desktop updater
- **iOS handler model**: IAP plugin commands use native async handlers (not ad-hoc `Task {}` wrappers) to reduce Swift concurrency allocator crashes seen during simulator testing
- **Simulator behavior**: by default, iOS simulator uses a crash-safe mock purchase/restore/status path in the plugin; set `DIARYX_IAP_SIMULATOR_REAL=1` in the Xcode scheme environment to force real StoreKit calls
- **Device packaging**: iOS target sets `SWIFT_STDLIB_TOOL_FLAGS=--source-libraries $(TOOLCHAIN_DIR)/usr/lib/swift-5.0/$(PLATFORM_NAME)` so `libswiftCore.dylib` and related Swift runtime libraries are embedded correctly for phone builds

The frontend detects the billing provider at runtime (`$lib/billing/platform.ts`):

- iOS Tauri and App Store macOS builds → Apple IAP (native StoreKit sheet)
- Web and direct desktop builds → Stripe checkout

```bash
# Normal dev (no IAP)
bun run tauri dev

# iOS dev with Apple/App Store feature flags
bun run tauri:ios

# App Store release build
cargo tauri build -- --features apple
```

Testing requires a StoreKit configuration file (`.storekit`) in Xcode for sandbox testing. For local sync-server testing with simulator mock transactions, set `APPLE_IAP_SKIP_SIGNATURE_VERIFY=1` on the server so mock JWS payloads can be decoded.

## Publishing

See [PUBLISHING.md](PUBLISHING.md) for the full guide to publishing to the App Store (iOS + macOS) and signing GitHub Releases.

## Platform Support

- macOS (Intel and Apple Silicon)
- Windows
- Linux
- iOS (via Tauri mobile)
- Android (via Tauri mobile)

Mobile platforms use platform-appropriate paths within app sandboxes. On iOS,
the sandbox container UUID changes between builds and reinstalls, so
`initialize_app` re-resolves any absolute workspace path stored in config by
extracting the folder name and joining it to the current `document_dir`. This
mirrors the same logic already present in `reinitialize_workspace` and prevents
`EPERM` errors when a dev build overwrites an older install.

On sandboxed macOS App Store builds, the default workspace now lives inside the
app container until the user explicitly picks an external folder. External
workspace picks are backed by security-scoped bookmarks so the app can keep
access across relaunches without requiring broad filesystem entitlements.
`reinitialize_workspace` and `initialize_app` also try to backfill a bookmark
when a workspace path is still accessible but missing from config, which helps
heal paths chosen by older builds once they are re-selected in the current
session.

## iCloud Drive (iOS)

The Tauri app includes `tauri-plugin-icloud` for iCloud Drive workspace storage on iOS. When enabled, the workspace is stored in the iCloud container directory (`iCloud.org.diaryx.app`) and syncs across devices automatically.

- **Plugin**: `tauri-plugin-icloud` (iOS only)
- **Container ID**: `iCloud.org.diaryx.app`
- **Feature flag**: `icloud` (included in `apple` umbrella)
- **Entitlement**: `com.apple.developer.ubiquity-container-identifiers` in the iOS entitlements plist
- **Config field**: `icloud_enabled` in `Config` — persists the user's choice across launches
- **Frontend**: iCloud settings appear in Settings > Data on Apple builds, with a toggle and sync status indicator
- **Migration**: Toggling iCloud on/off migrates workspace files between local Documents and the iCloud container using `FileManager.setUbiquitous` (upload) and `FileManager.copyItem` (download)
- **Sync status**: An `NSMetadataQuery` monitors the iCloud documents scope and emits `icloud-sync-status-changed` Tauri events to the frontend

**Note**: The iCloud container must also be registered in Apple Developer portal and the provisioning profile regenerated to include the iCloud entitlement.

On iOS, workspace files are stored in the app `Documents` directory and surfaced in the Files app under "On My iPhone" by enabling:

- `UIFileSharingEnabled`
- `LSSupportsOpeningDocumentsInPlace`

These keys are set via `src-tauri/Info.ios.plist` and merged through `bundle.iOS.infoPlist` in `src-tauri/tauri.conf.json`.
