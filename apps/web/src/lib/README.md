---
title: lib
description: Shared libraries and components
part_of: "[README](/apps/web/src/README.md)"
contents:
  - "[README](/apps/web/src/lib/auth/README.md)"
  - "[README](/apps/web/src/lib/backend/README.md)"
  - "[README](/apps/web/src/lib/components/README.md)"
  - "[README](/apps/web/src/lib/device/README.md)"
  - "[README](/apps/web/src/lib/extensions/README.md)"
  - "[README](/apps/web/src/lib/history/README.md)"
  - "[README](/apps/web/src/lib/hooks/README.md)"
  - "[README](/apps/web/src/lib/marketplace/README.md)"
  - "[README](/apps/web/src/lib/publish/README.md)"
  - "[README](/apps/web/src/lib/settings/README.md)"
  - "[README](/apps/web/src/lib/share/README.md)"
  - "[README](/apps/web/src/lib/sync/README.md)"
  - "[README](/apps/web/src/lib/storage/README.md)"
  - "[README](/apps/web/src/lib/stores/README.md)"
  - "[README](/apps/web/src/lib/wasm/README.md)"
attachments:
  - "[utils.ts](/apps/web/src/lib/utils.ts)"
  - "[windowDrag.ts](/apps/web/src/lib/windowDrag.ts)"
  - "[credentials.ts](/apps/web/src/lib/credentials.ts)"
  - "[mobileSwipe.ts](/apps/web/src/lib/mobileSwipe.ts)"
  - "[leftSidebarSelection.ts](/apps/web/src/lib/leftSidebarSelection.ts)"
  - "[wasm-stub.js](/apps/web/src/lib/wasm-stub.js)"
  - "[CommandPalette.svelte](/apps/web/src/lib/CommandPalette.svelte)"
  - "[Editor.svelte](/apps/web/src/lib/Editor.svelte)"
  - "[ExportDialog.svelte](/apps/web/src/lib/ExportDialog.svelte)"
  - "[LeftSidebar.svelte](/apps/web/src/lib/LeftSidebar.svelte)"
  - "[NewEntryModal.svelte](/apps/web/src/lib/NewEntryModal.svelte)"
  - "[MarketplaceDialog.svelte](/apps/web/src/lib/MarketplaceDialog.svelte)"
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

| Directory      | Purpose                                                                |
| -------------- | ---------------------------------------------------------------------- |
| `auth/`        | Authentication services and stores                                     |
| `backend/`     | Backend abstraction layer (WASM/Tauri)                                 |
| `components/`  | Reusable Svelte components                                             |
| `device/`      | Device identification                                                  |
| `extensions/`  | TipTap editor extensions                                               |
| `history/`     | Version history components                                             |
| `hooks/`       | Svelte hooks                                                           |
| `marketplace/` | Marketplace asset registry/apply logic (themes, typographies, bundles) |
| `publish/`     | Publishing and export components                                       |
| `settings/`    | Settings panel components                                              |
| `share/`       | Share session components                                               |
| `sync/`        | Sync plugin host-side adapters/services                                |
| `storage/`     | Storage abstraction                                                    |
| `stores/`      | Svelte stores                                                          |
| `wasm/`        | Built WASM module                                                      |

## Validation

Workspace naming, URL normalization, and publishing slug validation live in
`diaryx_core::utils::naming` (Rust) and are exposed to the frontend via
Commands (`ValidateWorkspaceName`, `ValidatePublishingSlug`,
`NormalizeServerUrl`). The typed wrappers are in
`backend/api.ts`. Frontend components call these instead of duplicating
validation logic locally.

## Add Workspace Dialog

`AddWorkspaceDialog.svelte` is the unified workspace creation dialog. It presents
two orthogonal dimensions:

- **Sync mode** (Local / Remote segmented toggle)
- **Content source** (From existing workspace / Import from ZIP / Start fresh)

The dialog uses staged progress updates during initialization
(`upload snapshot` -> `prepare local workspace state` -> `connect` -> `metadata sync`) so
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

Browser sync now loads the Extism sync plugin from
`/plugins/diaryx_sync.wasm` with a runtime compatibility check. If the file is
an older wasm-bindgen-flavored artifact, loading fails fast with a rebuild
instruction instead of surfacing a low-level Extism import resolution error.

Extism sync guest calls are serialized in
`plugins/extismBrowserLoader.ts` so browser transport callbacks and host
events cannot re-enter the guest concurrently and trip internal `RefCell`
borrows.

Browser host-side sync wiring lives in `plugins/extismBrowserLoader.ts` and
`sync/providerPluginCommands.ts` / `sync/workspaceProviderService.ts` so the
web app remains a plugin host, provider commands prefer plugin-owned
runtime/config state, and sync logic stays in the external sync plugin.

`App.svelte` only auto-opens the dialog after a fresh sign-in / magic-link
verification that still needs workspace bootstrap. It no longer reopens the
setup flow on every page reload for authenticated users who have not enabled
sync on the current client.

On Tauri, the dialog resets the location field when opened and derives a
fresh default path from the app document directory + workspace name when no
explicit location is provided, so "start fresh" flows don't reuse a previous
workspace folder accidentally.

For local->sync uploads, the snapshot root is resolved from the selected local
workspace's stored filesystem path when available (instead of ambient backend
path), preventing uploads from reading a different currently-open workspace.

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

The command palette can now be plugin-owned via a `UiContribution::CommandPalette`
surface contribution. When no plugin owns the surface, the built-in fallback is
limited to backup/import actions.

## Editor performance

`Editor.svelte` now preserves unsaved content across internal TipTap rebuilds
while avoiding full-document markdown serialization on every keystroke. Large
documents are serialized on demand for save/export/sync checks instead, which
keeps typing latency down in long notes.

## Sidebar Layout

- Left sidebar: built-in `Files` tab plus plugin-contributed tabs.
- Right sidebar: built-in `Properties` tab plus plugin-contributed tabs.
- Marketplace: opened from a dedicated modal surface
  (`MarketplaceDialog.svelte`): desktop `Dialog`, mobile `Drawer`.
  The desktop dialog now clamps to the available viewport height and keeps the
  marketplace body scrollable so small Tauri windows do not clip the surface.
- Marketplace tabs now own appearance customization (`Themes`, `Typography`, `Bundles`) in addition to plugin browsing, with curated + local registries for themes and typography presets.
- `mobileSwipe.ts` centralizes app-shell gesture gating so sidebar-open swipes are edge-triggered and modal/drawer or text-selection gestures do not leak through to the shell.
- Plugin sidebars are host-rendered with `components/PluginSidebarPanel.svelte`.
- Status-bar plugin items are host-rendered with `components/PluginStatusItems.svelte`, which only displays plugin-reported status and leaves plugin-specific actions to the plugin itself.
- `RightSidebar.svelte` resets collapse-button tooltip state when collapsing to
  prevent stale tooltip visibility when reopening the panel.
- `RightSidebar.svelte` now probes attachment local availability through
  attachment path resolution + file existence checks instead of reading full
  attachment bytes, keeping the properties panel responsive in media-heavy
  entries.
- Sidebar attachment preview clicks now reuse the shared attachment
  resolver/blob cache for previewable media instead of calling
  `GetAttachmentData` and creating a fresh preview blob on every open, which
  makes repeat previews effectively instant and keeps the first open on the
  binary read path.
- The attachment preview dialog now opens images with a cached thumbnail
  immediately when one already exists, then swaps to the full media when ready.
  Videos and audio use the same preview surface. On Tauri, the full-preview
  step prefers a native `asset:` URL for local verified media files and falls
  back to the blob resolver when native loading is unavailable.
- Attachment uploads and picker inserts now classify files as
  image/video/audio/file so previewable media insert directly into the editor
  instead of only images taking the fast embed path.
- Attachment/media markdown serialization now wraps destinations containing
  whitespace in angle brackets, so uploaded videos and other attachments with
  spaced filenames persist as valid CommonMark embeds instead of degrading to
  plain text on reload.
- Image nodes support Obsidian-style inline resize via `![alt|WIDTHxHEIGHT](src)`.
  The image context menu includes a **Resize** submenu with percentage presets
  (25%, 50%, 75%, 100%) and a custom size prompt. Dimensions are stored as
  `width`/`height` node attributes and round-trip through markdown.
- `LeftSidebar.svelte` dismisses the Settings-button tooltip on click so it
  does not remain visible after opening Settings, temporarily suppresses the
  tooltip while Settings is open/closing, and uses controlled tooltip open
  gating plus `ignoreNonKeyboardFocus` to prevent reopen on dialog focus
  restore and transition races. Settings/Marketplace footer tooltips also
  blur their triggers on dialog close and require one pointer-leave before
  opening again.
- `LeftSidebar.svelte` supports desktop multi-select (`Cmd`/`Ctrl` toggle,
  `Shift` range) with a small bulk-action bar. Sidebar delete requests are
  expanded to selected descendants and executed child-first so index entries
  with non-empty `contents` can be removed from the UI without manual
  leaf-by-leaf deletion.
- On Tauri desktop, `LeftSidebar.svelte` also exposes a context-menu action to
  reveal the selected entry in Finder/Explorer/the system file manager via the
  backend's opener-backed `revealInFileManager()` helper. The action is hidden
  on mobile because Tauri does not support reveal flows there.
- `windowDrag.ts` centralizes Tauri desktop window dragging for shared chrome
  surfaces. Sidebar/header/footer drag handlers use it and automatically skip
  interactive descendants such as buttons, links, inputs, and elements marked
  with `data-window-drag-exclude`.

## Plugin-Contributed Surfaces

Sync/share/history/publish behavior is plugin-contributed.
The web host keeps only generic infrastructure:

- Browser plugin runtime + typed command routing
- Generic iframe/component rendering
- Workspace tree/editor refresh from backend filesystem events

The host does not keep a web-specific CRDT bridge module.

Marketplace installs and removals now refresh plugin manifests and TipTap
editor extensions in-place on both browser and Tauri runtimes, so editor
features such as spoiler/math activate without a manual page reload. When a
plugin is removed mid-session, Diaryx keeps a preserve-only fallback extension
alive until the next reload so custom markdown syntax is not stripped from
open notes.

Left sidebar tree context menus can also be plugin-owned via
`UiContribution::ContextMenu { target: LeftSidebarTree, ... }`. When no plugin
owns this surface, the built-in fallback context menu is limited to
backup/import actions.

## Sidebar Tree Performance

`LeftSidebar.svelte` pre-groups validation errors by path for O(1) row lookups
instead of scanning the full error list per rendered node. This keeps folder
expand/collapse interactions responsive in larger workspaces.

The tree renderer also deduplicates children by `path` before keyed rendering,
so duplicate references from upstream data do not crash Svelte keyed `each`
blocks.

During file switches, `App.svelte` passes a pending `activeEntryPath` into
`LeftSidebar.svelte` so the newly clicked row highlights immediately even when
the backend is still resolving the next entry (for example, while attachment
loads are being canceled).
