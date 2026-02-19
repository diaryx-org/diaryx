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
  - '[AddWorkspaceDialog.svelte](/apps/web/src/lib/AddWorkspaceDialog.svelte)'
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

## Add Workspace Dialog

`AddWorkspaceDialog.svelte` is the unified workspace creation dialog. It presents
two orthogonal dimensions:

- **Sync mode** (Local / Remote segmented toggle)
- **Content source** (From existing workspace / Import from ZIP / Start fresh)

The dialog uses staged progress updates during initialization
(`upload snapshot` -> `prepare local CRDT` -> `connect` -> `metadata sync`) so
users see visible forward motion even when backend operations don't emit
granular file progress for small workspaces.

`WorkspaceSelector.svelte` opens `AddWorkspaceDialog.svelte` from the
`New workspace` button so workspace initialization always goes through the same
setup flow instead of inline naming/creation.

Both local and synced workspace creation prompt for a workspace name and
automatically create a root index file during initialization.

The dialog's local->sync upload path copies local workspace files to the server
(snapshot upload) and keeps local files on device; it does not delete or move
local data.

`App.svelte` routes the editor empty-workspace `Initialize workspace` action
to the same setup flow (`AddWorkspaceDialog.svelte`) instead of exposing separate
`Create Root Index` and `Import from ZIP` buttons in the editor area.

## Command Palette Dialog Sequencing

`CommandPalette.svelte` closes the palette and awaits a Svelte `tick()` before
running the selected command action. This prevents overlapping Radix dialogs
when a command opens another modal (for example, `New Entry`).

`NewEntryModal.svelte` also guards its parent-picker root expansion effect so it
does not continuously rewrite `pickerExpanded`. This avoids reactive update-loop
errors that can leave overlapping dialogs/focus traps on screen.
