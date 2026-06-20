---
title: AGENTS
description: Instructions for AI Agents
author: adammharris
updated: 2026-06-20T02:12:17Z
part_of: '[Diaryx](/Diaryx.md)'
audience:
- agents
---
# Instructions for AI agents

Always read the relevant docs before making changes, and update the relevant docs after making changes. A tree is shown below for reference, with the title, description, and filepath of each file shown.

## Workspace Overview

```workspace-index
Diaryx - README/repo for the Diaryx project - README.md
├── AGENTS - Instructions for AI Agents - AGENTS.md
├── Contributing to Diaryx - A guide for making contributions in the Diaryx repo - Contributing to Diaryx.md
├── LICENSE - PolyForm Shield License 1.0.0 - LICENSE.md
├── PHILOSOPHY - PHILOSOPHY.md
├── Privacy Policy - Privacy Policy for Diaryx - privacy.md
├── apps - GUI frontends for Diaryx - apps/README.md
│   ├── web - Svelte + Tiptap frontend for Diaryx - apps/web/README.md
│   │   ├── web src - Source code for the Diaryx web application - apps/web/src/README.md
│   │   │   ├── Controllers - Controller logic for UI actions - apps/web/src/controllers/README.md
│   │   │   ├── lib - Shared libraries and components - apps/web/src/lib/README.md
│   │   │   │   ├── Auth - Authentication services and stores - apps/web/src/lib/auth/README.md
│   │   │   │   ├── Backend - Backend abstraction layer for WASM and Tauri - apps/web/src/lib/backend/README.md
│   │   │   │   ├── Components - Reusable Svelte components - apps/web/src/lib/components/README.md
│   │   │   │   │   └── UI Components - shadcn-svelte based UI primitives - apps/web/src/lib/components/ui/README.md
│   │   │   │   ├── Device - Device identification - apps/web/src/lib/device/README.md
│   │   │   │   ├── Extensions - TipTap editor extensions - apps/web/src/lib/extensions/README.md
│   │   │   │   ├── Hooks - Svelte hooks - apps/web/src/lib/hooks/README.md
│   │   │   │   ├── Marketplace - Marketplace registries and bundle apply execution - apps/web/src/lib/marketplace/README.md
│   │   │   │   ├── Namespace - Namespace management services and host-side UI components - apps/web/src/lib/namespace/README.md
│   │   │   │   ├── Publish - Publishing and export UI wiring - apps/web/src/lib/publish/README.md
│   │   │   │   ├── Settings - Settings panel components - apps/web/src/lib/settings/README.md
│   │   │   │   ├── Share - Legacy share/publish panel module - apps/web/src/lib/share/README.md
│   │   │   │   ├── Storage - Storage abstraction layer - apps/web/src/lib/storage/README.md
│   │   │   │   ├── Lib Stores - Svelte stores for UI preferences - apps/web/src/lib/stores/README.md
│   │   │   │   └── diaryx_wasm - WASM bindings for diaryx_core - apps/web/src/lib/wasm/README.md
│   │   │   ├── Models - Stores and services for application state - apps/web/src/models/README.md
│   │   │   │   ├── Services - Business logic services - apps/web/src/models/services/README.md
│   │   │   │   └── Stores - Svelte stores for reactive state - apps/web/src/models/stores/README.md
│   │   │   └── Views - View components - apps/web/src/views/README.md
│   │   │       ├── Editor Views - Editor-related view components - apps/web/src/views/editor/README.md
│   │   │       ├── Layout Views - Layout components - apps/web/src/views/layout/README.md
│   │   │       ├── Marketplace Views - Marketplace panels and plugin/theme browsing views - apps/web/src/views/marketplace/README.md
│   │   │       ├── Shared Views - Shared view components - apps/web/src/views/shared/README.md
│   │   │       └── Sidebar Views - Sidebar components - apps/web/src/views/sidebar/README.md
│   │   ├── web worker - Cloudflare Worker entrypoint for app.diaryx.org - apps/web/worker/README.md
│   │   └── TipTap Custom Extensions - Guide to creating custom TipTap extensions with markdown support - apps/web/docs/tiptap-custom-extensions.md
│   └── tauri - Web app + native backend - apps/tauri/README.md
├── crates - Cargo crates for Diaryx - crates/README.md
│   ├── crossfs - std::fs / tokio::fs that also runs on OPFS, IndexedDB, and the File System Access API in the browser - crates/crossfs/README.md
│   ├── diaryx - CLI frontend - crates/diaryx/README.md
│   │   └── diaryx src - Source code for the Diaryx CLI application - crates/diaryx/src/README.md
│   │       └── Command-line module - The main CLI command implementation module - crates/diaryx/src/cli/README.md
│   │           └── Navigation TUI module - Interactive TUI for navigating workspace hierarchy - crates/diaryx/src/cli/nav/README.md
│   ├── diaryx_server - Platform-agnostic server core for Diaryx cloud adapters - crates/diaryx_server/README.md
│   │   └── diaryx_server src - Platform-agnostic core modules for Diaryx server adapters - crates/diaryx_server/src/README.md
│   ├── diaryx_core - Core library shared by Diaryx clients - crates/diaryx_core/README.md
│   │   └── diaryx_core src - Source code for the core Diaryx library - crates/diaryx_core/src/README.md
│   │       ├── Entry module - Entry manipulation functionality - crates/diaryx_core/src/entry/README.md
│   │       ├── Filesystem module - Filesystem abstraction layer - crates/diaryx_core/src/fs/README.md
│   │       ├── crates/diaryx_core/src/plugin/README.md
│   │       ├── Utils module - Utility functions for date and path handling - crates/diaryx_core/src/utils/README.md
│   │       └── Workspace module - Workspace tree organization - crates/diaryx_core/src/workspace/README.md
│   ├── diaryx_wasm - WASM bindings for diaryx_core - crates/diaryx_wasm/README.md
│   ├── diaryx_sync_server - Sync server used by frontends - crates/diaryx_sync_server/README.md
│   │   └── diaryx_sync_server src - Source code for the sync server - crates/diaryx_sync_server/src/README.md
│   │       ├── Auth module - Authentication middleware and magic link handling - crates/diaryx_sync_server/src/auth/README.md
│   │       ├── Database module - SQLite database schema and repository - crates/diaryx_sync_server/src/db/README.md
│   │       ├── Email module - SMTP email sending for magic links - crates/diaryx_sync_server/src/email/README.md
│   │       └── Handlers module - HTTP route handlers - crates/diaryx_sync_server/src/handlers/README.md
│   ├── crates/diaryx_extism/README.md
│   └── crates/plugins/diaryx_plugin_sdk/README.md
├── ROADMAP - The plan for future Diaryx features - ROADMAP.md
└── Terms of Service - Terms of Service for Diaryx - terms.md
```

## Entry Points

Read the root README.md first. For specific projects, use these entry points:


| Project          | Entry point                         |
| ---------------- | ----------------------------------- |
| Entire workspace | README.md                           |
| Core library     | crates/diaryx_core/README.md        |
| Native adapters  | crates/diaryx_native/README.md      |
| CLI              | crates/diaryx/README.md             |
| Web app          | apps/web/README.md                  |
| Tauri app        | apps/tauri/README.md                |
| WASM bindings    | crates/diaryx_wasm/README.md        |
| Sync server      | crates/diaryx_sync_server/README.md |
| Extism host      | crates/diaryx_extism/README.md      |
| Plugin SDK       | crates/plugins/diaryx_plugin_sdk/README.md    |
| Sync plugin      | crates/plugins/diaryx_sync_extism/README.md   |
| Publish plugin   | crates/plugins/diaryx_publish_extism/README.md |


## Commands

Note: You may also access these commands from the Nix flake via `nix develop -c <COMMAND>`, or `nix develop` and afterward run the command normally.

```bash
# Install the pre-commit hook (one-time, per clone)
cargo xtask install-hooks

# Build all crates
cargo build

# Run tests
cargo test

# Run CLI
cargo run --bin diaryx -- <args>

# Build WASM
cargo xtask build-wasm
# Or, since this command is included in package.json...
cd apps/web && bun run build

# Web dev server
cd apps/web && bun run dev

# Tauri dev (macOS desktop; use `ios` for iOS, `--dev-ipc` for the debug HTTP listener)
cargo xtask tauri macos
```

### xtask subcommands

Project automation lives in `cargo xtask`. Run `cargo xtask help` for full usage and flags.

| Command | Purpose |
| --- | --- |
| `build-wasm [--panic-hook]` | Build `crates/diaryx_wasm` for `apps/web` (wasm-pack + wasm-opt). |
| `build-plugin <name>` | Build a plugin WASM (`--release` for size-optimized). |
| `check [--fix]` | Run `cargo fmt` + `cargo clippy` concurrently with `svelte-check`. |
| `clean [--dry-run]` | `cargo clean` plus removal of stray nested `target/` dirs. |
| `install-hooks [--force]` | Install `.git/hooks/pre-commit` → `cargo xtask pre-commit`. |
| `pre-commit [--all]` | Run the pre-commit checks (invoked by the git hook). |
| `publish-ios` | Build the iOS App Store export and upload via altool (macOS only). |
| `publish-macos <build>` | Build, sign, package, and upload the macOS App Store `.pkg` (macOS only). |
| `release-plugin <name> [--upload]` | Build a release WASM + `dist/` artifact; with `--upload`, cut a GitHub Release + open a plugin-registry PR. |
| `sync-bindings` | Sync ts-rs bindings into `apps/web/src/lib/backend/generated/`. |
| `sync-marketplace` | Fetch marketplace registries from the production CDN. |
| `sync-versions` | Propagate `README.md` version → `Cargo.toml` / `tauri.conf.json` / `package.json` / `flake.nix`. |
| `tauri <subcommand>` | Tauri builds: `macos`, `ios`, `render-updater-config`. |
| `update-agents-index` | Refresh the workspace tree in `AGENTS.md`. |

## Not Documented

Read these files directly when needed:

- CI/workflows: `.github/workflows/*.yml`
- Pre-commit hook: `cargo xtask pre-commit` (install via `cargo xtask install-hooks`)
