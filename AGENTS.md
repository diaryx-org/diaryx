---
title: AGENTS
description: Instructions for AI Agents
author: adammharris
updated: 2026-03-04T01:56:16Z
part_of: '[README](/README.md)'
---
# Instructions for AI agents

Always read the relevant docs before making changes, and update the relevant docs after making changes. A tree is shown below for reference, with the title, description, and filepath of each file shown.

## Workspace Overview

```workspace-index
Diaryx Monorepo - README/repo for the Diaryx project - README.md
├── AGENTS - Instructions for AI Agents - AGENTS.md
├── CONTRIBUTING - A guide for making contributions in the Diaryx repo - CONTRIBUTING.md
├── LICENSE - PolyForm Shield License 1.0.0 - LICENSE.md
├── apps - GUI frontends for Diaryx - apps/README.md
│   ├── web - Svelte + Tiptap frontend for Diaryx - apps/web/README.md
│   │   ├── web src - Source code for the Diaryx web application - apps/web/src/README.md
│   │   │   ├── Controllers - Controller logic for UI actions - apps/web/src/controllers/README.md
│   │   │   ├── lib - Shared libraries and components - apps/web/src/lib/README.md
│   │   │   │   ├── Auth - Authentication services and stores - apps/web/src/lib/auth/README.md
│   │   │   │   ├── Backend - Backend abstraction layer for WASM and Tauri - apps/web/src/lib/backend/README.md
│   │   │   │   ├── Components - Reusable Svelte components - apps/web/src/lib/components/README.md
│   │   │   │   │   └── UI Components - shadcn-svelte based UI primitives - apps/web/src/lib/components/ui/README.md
│   │   │   │   ├── CRDT - Legacy placeholder for removed web CRDT host layer - apps/web/src/lib/crdt/README.md
│   │   │   │   ├── Device - Device identification - apps/web/src/lib/device/README.md
│   │   │   │   ├── Extensions - TipTap editor extensions - apps/web/src/lib/extensions/README.md
│   │   │   │   ├── History - Version history components - apps/web/src/lib/history/README.md
│   │   │   │   ├── Hooks - Svelte hooks - apps/web/src/lib/hooks/README.md
│   │   │   │   ├── Publish - Publishing and export UI components - apps/web/src/lib/publish/README.md
│   │   │   │   ├── Settings - Settings panel components - apps/web/src/lib/settings/README.md
│   │   │   │   ├── Share - Legacy share/publish panel module - apps/web/src/lib/share/README.md
│   │   │   │   ├── Storage - Storage abstraction layer - apps/web/src/lib/storage/README.md
│   │   │   │   ├── Lib Stores - Svelte stores for UI preferences - apps/web/src/lib/stores/README.md
│   │   │   │   └── diaryx_wasm - WASM bindings for diaryx_core - apps/web/src/lib/wasm/README.md
│   │   │   │       └── diaryx_wasm src - Source code for WASM bindings - crates/diaryx_wasm/src/README.md
│   │   │   ├── Models - Stores and services for application state - apps/web/src/models/README.md
│   │   │   │   ├── Services - Business logic services - apps/web/src/models/services/README.md
│   │   │   │   └── Stores - Svelte stores for reactive state - apps/web/src/models/stores/README.md
│   │   │   ├── Views - View components - apps/web/src/views/README.md
│   │   │   │   ├── Editor Views - Editor-related view components - apps/web/src/views/editor/README.md
│   │   │   │   ├── Layout Views - Layout components - apps/web/src/views/layout/README.md
│   │   │   │   ├── Shared Views - Shared view components - apps/web/src/views/shared/README.md
│   │   │   │   └── Sidebar Views - Sidebar components - apps/web/src/views/sidebar/README.md
│   │   │   └── LICENSE - PolyForm Shield License 1.0.0 - apps/web/src/LICENSE.md
│   │   └── TipTap Custom Extensions - Guide to creating custom TipTap extensions with markdown support - apps/web/docs/tiptap-custom-extensions.md
│   ├── tauri - Web app + native backend - apps/tauri/README.md
│   └── apple - Native SwiftUI app for Diaryx using WKWebView + TipTap - apps/apple/README.md
├── crates - Cargo crates for Diaryx - crates/README.md
│   ├── diaryx - CLI frontend - crates/diaryx/README.md
│   │   └── diaryx src - Source code for the Diaryx CLI application - crates/diaryx/src/README.md
│   │       └── Command-line module - The main CLI command implementation module - crates/diaryx/src/cli/README.md
│   │           ├── Navigation TUI module - Interactive TUI for navigating workspace hierarchy - crates/diaryx/src/cli/nav/README.md
│   │           └── Sync CLI module - CLI commands for workspace synchronization - crates/diaryx/src/cli/sync/README.md
│   ├── diaryx_apple - UniFFI bridge crate for Apple clients - crates/diaryx_apple/README.md
│   │   └── diaryx_apple src - Source code for the Apple UniFFI bridge crate - crates/diaryx_apple/src/README.md
│   ├── diaryx_core - Core library shared by Diaryx clients - crates/diaryx_core/README.md
│   │   └── diaryx_core src - Source code for the core Diaryx library - crates/diaryx_core/src/README.md
│   │       ├── Entry module - Entry manipulation functionality - crates/diaryx_core/src/entry/README.md
│   │       ├── Filesystem module - Filesystem abstraction layer - crates/diaryx_core/src/fs/README.md
│   │       ├── crates/diaryx_core/src/plugin/README.md
│   │       ├── Publish module - ContentProvider trait — shared publish abstractions - crates/diaryx_core/src/publish/README.md
│   │       ├── Utils module - Utility functions for date and path handling - crates/diaryx_core/src/utils/README.md
│   │       ├── Workspace module - Workspace tree organization - crates/diaryx_core/src/workspace/README.md
│   │       └── Import module - Import external formats into Diaryx entries - crates/diaryx_core/src/import/README.md
│   ├── crates/diaryx_daily/README.md
│   ├── crates/diaryx_daily_extism/README.md
│   ├── diaryx_import_extism - Extism guest plugin for Diaryx import functionality - crates/diaryx_import_extism/README.md
│   ├── diaryx_ai_extism - Extism AI assistant plugin for Diaryx - crates/diaryx_ai_extism/README.md
│   ├── diaryx_publish - Publishing pipeline for Diaryx workspaces — converts markdown to HTML - crates/diaryx_publish/README.md
│   ├── crates/diaryx_publish_extism/README.md
│   ├── diaryx_wasm - WASM bindings for diaryx_core - crates/diaryx_wasm/README.md
│   │   └── diaryx_wasm src - Source code for WASM bindings - crates/diaryx_wasm/src/README.md
│   ├── diaryx_sync_server - Sync server used by frontends - crates/diaryx_sync_server/README.md
│   │   └── diaryx_sync_server src - Source code for the sync server - crates/diaryx_sync_server/src/README.md
│   │       ├── Auth module - Authentication middleware and magic link handling - crates/diaryx_sync_server/src/auth/README.md
│   │       ├── Database module - SQLite database schema and repository - crates/diaryx_sync_server/src/db/README.md
│   │       ├── Email module - SMTP email sending for magic links - crates/diaryx_sync_server/src/email/README.md
│   │       └── Handlers module - HTTP route handlers - crates/diaryx_sync_server/src/handlers/README.md
│   ├── diaryx_sync - crates/diaryx_sync/README.md
│   ├── crates/diaryx_storage_s3_extism/README.md
│   └── crates/diaryx_storage_gdrive_extism/README.md
├── ROADMAP - The plan for future Diaryx features - ROADMAP.md
└── Scripts - scripts/scripts.md
```

## Entry Points

Read the root README.md first. For specific projects, use these entry points:


| Project          | Entry point                         |
| ---------------- | ----------------------------------- |
| Entire workspace | README.md                           |
| Core library     | crates/diaryx_core/README.md        |
| Daily domain     | crates/diaryx_daily/README.md       |
| Templating       | crates/diaryx_templating/README.md  |
| Publish pipeline | crates/diaryx_publish/README.md     |
| CLI              | crates/diaryx/README.md             |
| Web app          | apps/web/README.md                  |
| Tauri app        | apps/tauri/README.md                |
| WASM bindings    | crates/diaryx_wasm/README.md        |
| Sync engine      | crates/diaryx_sync/README.md        |
| Sync server      | crates/diaryx_sync_server/README.md |
| Extism plugins   | crates/diaryx_extism/README.md      |
| Daily plugin guest | crates/diaryx_daily_extism/README.md |
| Import plugin guest | crates/diaryx_import_extism/README.md |
| Templating plugin guest | crates/diaryx_templating_extism/README.md |
| S3 storage       | crates/diaryx_storage_s3_extism/README.md |
| GDrive storage   | crates/diaryx_storage_gdrive_extism/README.md |


## Commands

Note: You may also access these commands from the Nix flake via `nix develop -c <COMMAND>`, or `nix develop` and afterward run the command normally.

```bash
# Build all crates
cargo build

# Run tests
cargo test

# Run CLI
cargo run --bin diaryx -- <args>

# Build WASM
./scripts/build-wasm.sh
# Or, since this script is included in package.json...
cd apps/web && bun run build

# Web dev server
cd apps/web && bun run dev

# Tauri dev
cd apps/tauri && cargo tauri dev
```

## Not Documented

Read these files directly when needed:

- CI/workflows: `.github/workflows/*.yml`
- Pre-commit config: `.pre-commit-config.yaml`
