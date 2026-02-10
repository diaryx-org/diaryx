---
title: lib
description: Shared libraries and components
part_of: '[README](/apps/web/src/README.md)'
contents:
  - '[README](/apps/web/src/lib/auth/README.md)'
  - '[README](/apps/web/src/lib/backend/README.md)'
  - '[README](/apps/web/src/lib/components/README.md)'
  - '[README](/apps/web/src/lib/crdt/README.md)'
  - '[README](/apps/web/src/lib/device/README.md)'
  - '[README](/apps/web/src/lib/extensions/README.md)'
  - '[README](/apps/web/src/lib/history/README.md)'
  - '[README](/apps/web/src/lib/hooks/README.md)'
  - '[README](/apps/web/src/lib/settings/README.md)'
  - '[README](/apps/web/src/lib/share/README.md)'
  - '[README](/apps/web/src/lib/storage/README.md)'
  - '[README](/apps/web/src/lib/stores/README.md)'
  - '[README](/apps/web/src/lib/wasm/README.md)'
attachments:
  - '[utils.ts](/apps/web/src/lib/utils.ts)'
  - '[credentials.ts](/apps/web/src/lib/credentials.ts)'
  - '[wasm-stub.js](/apps/web/src/lib/wasm-stub.js)'
  - '[CommandPalette.svelte](/apps/web/src/lib/CommandPalette.svelte)'
  - '[Editor.svelte](/apps/web/src/lib/Editor.svelte)'
  - '[ExportDialog.svelte](/apps/web/src/lib/ExportDialog.svelte)'
  - '[LeftSidebar.svelte](/apps/web/src/lib/LeftSidebar.svelte)'
  - '[NewEntryModal.svelte](/apps/web/src/lib/NewEntryModal.svelte)'
  - '[RightSidebar.svelte](/apps/web/src/lib/RightSidebar.svelte)'
  - '[SettingsDialog.svelte](/apps/web/src/lib/SettingsDialog.svelte)'
  - '[SyncSetupWizard.svelte](/apps/web/src/lib/SyncSetupWizard.svelte)'
  - '[SyncStatusIndicator.svelte](/apps/web/src/lib/SyncStatusIndicator.svelte)'
exclude:
  - '*.lock'
---

# Lib

Shared libraries, components, and utilities for the web application.

## Structure

| Directory | Purpose |
|-----------|---------|
| `auth/` | Authentication services and stores |
| `backend/` | Backend abstraction layer (WASM/Tauri) |
| `components/` | Reusable Svelte components |
| `crdt/` | CRDT synchronization bridge |
| `device/` | Device identification |
| `extensions/` | TipTap editor extensions |
| `history/` | Version history components |
| `hooks/` | Svelte hooks |
| `settings/` | Settings panel components |
| `share/` | Share session components |
| `storage/` | Storage abstraction |
| `stores/` | Svelte stores |
| `wasm/` | Built WASM module |

## Sync Setup Progress

`SyncSetupWizard.svelte` uses staged progress updates during initialization
(`upload snapshot` -> `prepare local CRDT` -> `connect` -> `metadata sync`) so
users see visible forward motion even when backend operations don't emit
granular file progress for small workspaces.
