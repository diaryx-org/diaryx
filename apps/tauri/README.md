---
title: tauri
description: Web app + native backend
author: adammharris
audience:
- public
- developers
part_of: '[README](/apps/README.md)'
updated: 2026-03-29T19:40:38-06:00
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

For iOS dev builds, `bun run tauri:ios` now runs `bun run clean:ios-swift-cache`
first. This clears stale `swift-rs` module artifacts for Tauri and Tauri plugin
build scripts under `target/` that can break builds after moving the repository
to a new absolute path.

## Tauri Commands

All IPC commands are defined in `src-tauri/src/commands.rs` and registered in `src-tauri/src/lib.rs`.

### Validation Commands

The validation system checks workspace link integrity and can automatically fix issues.


| Command                     | Description                            |
| --------------------------- | -------------------------------------- |
| `validate_workspace`        | Validate entire workspace from root    |
| `validate_file`             | Validate a single file's links         |
| `fix_broken_part_of`        | Remove broken `part_of` reference      |
| `fix_broken_contents_ref`   | Remove broken `contents` reference     |
| `fix_broken_attachment`     | Remove broken `attachments` reference  |
| `fix_non_portable_path`     | Normalize non-portable paths           |
| `fix_unlisted_file`         | Add file to index's contents           |
| `fix_orphan_binary_file`    | Add binary file to index's attachments |
| `fix_missing_part_of`       | Set missing `part_of` property         |
| `fix_all_validation_issues` | Fix all errors and fixable warnings    |


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
