---
title: tauri
description: Web app + native backend
author: adammharris
audience:
- public
- developers
part_of: '[README](/apps/README.md)'
---

# Diaryx Tauri

The Tauri backend for Diaryx, providing native filesystem access for the web frontend.

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

| Category       | Commands                                                                          |
| -------------- | --------------------------------------------------------------------------------- |
| Workspace      | `get_workspace_tree`, `get_filesystem_tree`, `create_workspace`                   |
| Entries        | `get_entry`, `save_entry`, `create_entry`, `delete_entry`, `move_entry`           |
| Entries (cont) | `attach_entry_to_parent`, `convert_to_index`, `convert_to_leaf`                   |
| Entries (cont) | `create_child_entry`, `rename_entry`, `ensure_daily_entry`                        |
| Frontmatter    | `get_frontmatter`, `set_frontmatter_property`, `remove_frontmatter_property`      |
| Attachments    | `get_attachments`, `upload_attachment`, `delete_attachment`                       |
| Search         | `search_workspace`                                                                |
| Export         | `get_available_audiences`, `plan_export`, `export_to_memory`, `export_to_html`    |
| Backup         | `backup_workspace`, `restore_workspace`, `backup_to_s3`, `backup_to_google_drive` |
| Import         | `import_from_zip`, `pick_and_import_zip`                                          |

## Sync Transport

The Tauri app uses `SyncClient` from `diaryx_core` with `TokioConnector` (tokio-tungstenite + rustls) for WebSocket sync. `SyncClient` is generic over `TransportConnector`, allowing the transport to be swapped for platform-specific implementations (e.g., Apple's `URLSessionWebSocketTask` on iOS via a custom connector).

When native sync starts, the backend re-applies the current workspace root to
the shared `Diaryx` instance so sync path canonicalization reliably strips
absolute workspace prefixes before constructing body document IDs.

## Apple IAP (In-App Purchases)

The Tauri app includes `tauri-plugin-iap` for StoreKit 2 integration on iOS and macOS (Mac App Store). This enables native subscription purchasing through the App Store.

- **Plugin**: `tauri-plugin-iap` v0.8 (StoreKit 2, iOS 15+/macOS 13+)
- **Product ID**: `diaryx_plus_monthly` (configured in App Store Connect)
- **Capabilities**: `iap:default` in both `mobile.json` and `default.json`
- **Feature flag**: `iap` — the plugin is behind `--features iap` to avoid Swift bridge linker issues during dev
- **iOS handler model**: IAP plugin commands use native async handlers (not ad-hoc `Task {}` wrappers) to reduce Swift concurrency allocator crashes seen during simulator testing
- **Simulator behavior**: by default, iOS simulator uses a crash-safe mock purchase/restore/status path in the plugin; set `DIARYX_IAP_SIMULATOR_REAL=1` in the Xcode scheme environment to force real StoreKit calls
- **Device packaging**: iOS target sets `SWIFT_STDLIB_TOOL_FLAGS=--source-libraries $(TOOLCHAIN_DIR)/usr/lib/swift-5.0/$(PLATFORM_NAME)` so `libswiftCore.dylib` and related Swift runtime libraries are embedded correctly for phone builds

The frontend detects the billing provider at runtime (`$lib/billing/platform.ts`):
- iOS/macOS Tauri → Apple IAP (native StoreKit sheet)
- Web/Desktop (non-MAS) → Stripe checkout

```bash
# Normal dev (no IAP)
bun run tauri dev

# App Store release build with IAP
bun run tauri:iap
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

Mobile platforms use platform-appropriate paths within app sandboxes.

On iOS, workspace files are stored in the app `Documents` directory and surfaced in the Files app under "On My iPhone" by enabling:

- `UIFileSharingEnabled`
- `LSSupportsOpeningDocumentsInPlace`

These keys are set via `src-tauri/Info.ios.plist` and merged through `bundle.iOS.infoPlist` in `src-tauri/tauri.conf.json`.
