---
title: lib
description: Shared libraries and components
part_of: "[README](/apps/web/src/README.md)"
contents:
  - "[README](/apps/web/src/lib/auth/README.md)"
  - "[README](/apps/web/src/lib/backend/README.md)"
  - "[README](/apps/web/src/lib/components/README.md)"
  - "[README](/apps/web/src/lib/crdt/README.md)"
  - "[README](/apps/web/src/lib/device/README.md)"
  - "[README](/apps/web/src/lib/extensions/README.md)"
  - "[README](/apps/web/src/lib/history/README.md)"
  - "[README](/apps/web/src/lib/hooks/README.md)"
  - "[README](/apps/web/src/lib/publish/README.md)"
  - "[README](/apps/web/src/lib/settings/README.md)"
  - "[README](/apps/web/src/lib/share/README.md)"
  - "[README](/apps/web/src/lib/storage/README.md)"
  - "[README](/apps/web/src/lib/stores/README.md)"
  - "[README](/apps/web/src/lib/wasm/README.md)"
attachments:
  - "[utils.ts](/apps/web/src/lib/utils.ts)"
  - "[credentials.ts](/apps/web/src/lib/credentials.ts)"
  - "[wasm-stub.js](/apps/web/src/lib/wasm-stub.js)"
  - "[CommandPalette.svelte](/apps/web/src/lib/CommandPalette.svelte)"
  - "[Editor.svelte](/apps/web/src/lib/Editor.svelte)"
  - "[ExportDialog.svelte](/apps/web/src/lib/ExportDialog.svelte)"
  - "[LeftSidebar.svelte](/apps/web/src/lib/LeftSidebar.svelte)"
  - "[NewEntryModal.svelte](/apps/web/src/lib/NewEntryModal.svelte)"
  - "[RightSidebar.svelte](/apps/web/src/lib/RightSidebar.svelte)"
  - "[SettingsDialog.svelte](/apps/web/src/lib/SettingsDialog.svelte)"
  - "[AddWorkspaceDialog.svelte](/apps/web/src/lib/AddWorkspaceDialog.svelte)"
  - "[components/PluginSidebarPanel.svelte](/apps/web/src/lib/components/PluginSidebarPanel.svelte)"
  - "[components/PluginStatusItems.svelte](/apps/web/src/lib/components/PluginStatusItems.svelte)"
exclude:
  - "*.lock"
---

# Lib

Shared libraries, components, and utilities for the web application.

## Structure

| Directory | Purpose |
| --- | --- |
| `auth/` | Authentication services and stores |
| `backend/` | Backend abstraction layer (WASM/Tauri) |
| `components/` | Reusable Svelte components and generic plugin renderers |
| `crdt/` | Migration note only (web CRDT bridge removed) |
| `device/` | Device identification |
| `extensions/` | TipTap editor extensions |
| `history/` | Version history UI components |
| `hooks/` | Svelte hooks |
| `publish/` | Publishing and export surfaces |
| `settings/` | Settings dialog modules |
| `share/` | Legacy publish/share-adjacent module (no host share session UI) |
| `storage/` | Local workspace registry + storage adapters |
| `stores/` | UI-oriented stores |
| `wasm/` | Built WASM module |

## Sync Architecture

Web no longer embeds app-level CRDT/sync bridge code. The host keeps generic
plugin infrastructure and filesystem event handling, while sync ownership is in
`diaryx_sync` (`diaryx_sync_extism` runtime on web).

Key host pieces:

- `PluginSidebarPanel.svelte` and `PluginIframe.svelte` for plugin UI surfaces
- `PluginStatusItems.svelte` for generic status-bar contribution rendering
- `SettingsDialog.svelte` plugin settings tab rendering (`Iframe` + declarative)
- `workspace/switchWorkspace.ts` for backend lifecycle during workspace switches

## Sidebar Layout

- Left sidebar: built-in Files tab + plugin-contributed sidebar tabs
- Right sidebar: built-in Properties tab + plugin-contributed sidebar tabs
- Sync share/snapshots/history surfaces are plugin-rendered, not host-builtins

## Workspace Provider Flows

`AddWorkspaceDialog.svelte`, `WorkspaceSelector.svelte`, and
`settings/WorkspaceManagement.svelte` use `sync/workspaceProviderService.ts`,
which calls plugin commands (`GetProviderStatus`, `ListRemoteWorkspaces`,
`LinkWorkspace`, `UnlinkWorkspace`, `DownloadWorkspace`) and keeps local
workspace registry updates in the host.

## iOS Editor Toolbar

`Editor.svelte` switches to the native iOS toolbar when running in Tauri on iOS.
In that mode, web `FloatingMenu`/`BubbleMenu` UI is not required for editor
initialization; the editor must still mount even when those menu elements are
not rendered.
