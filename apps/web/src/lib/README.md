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

## Validation

Workspace naming, URL normalization, and publishing slug validation live in
`diaryx_core::utils::naming` (Rust) and are exposed to the frontend via
Commands (`ValidateWorkspaceName`, `ValidatePublishingSlug`,
`NormalizeServerUrl`, `ToWebSocketSyncUrl`). The typed wrappers are in
`backend/api.ts`. Frontend components call these instead of duplicating
validation logic locally.

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

Browser sync now loads the Extism plugin from
`/plugins/diaryx_sync.wasm` with a runtime compatibility check. If the file is
an older wasm-bindgen-flavored artifact, loading fails fast with a rebuild
instruction instead of surfacing a low-level Extism import resolution error.

Extism sync guest calls are serialized in `sync/extismSyncPlugin.ts` to avoid
re-entrant guest invocations, which can panic on internal `RefCell` borrows
when multiple sync events race in the browser.

Browser host functions in `sync/hostFunctions.ts` now bridge guest filesystem
calls (`host_read_file`, `host_file_exists`, etc.) to async backend commands,
so Extism sync metadata/materialization paths can read/write real workspace
files instead of falling back to no-op stubs.

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

## Sidebar Layout

The **left sidebar** (`LeftSidebar.svelte`) contains workspace-level concerns:
- **Files** tab: workspace tree, problems panel
- **Share** tab: live collaboration and publishing (`ShareTab.svelte`)
- **Snapshots** tab: git snapshot history (`GitHistoryPanel.svelte`)

The `WorkspaceSelector` and `AudienceFilter` sit above all tabs and are always visible.

The **right sidebar** (`RightSidebar.svelte`) contains file-level concerns:
- **Props** tab: frontmatter properties, attachments, audience, workspace config
- **History** tab: CRDT change history for the current file

Share and snapshots were moved from the right sidebar to the left sidebar because
they are workspace-level operations, not file-level.

## Plugin-Declared Sync UI

Sync/share/history surfaces are now declared by plugin manifest contributions
from the runtime `sync` Extism plugin (`diaryx_sync_extism`), with host-rendered
Svelte components mapped by built-in component IDs:

- `sync.share` -> `ShareTab`
- `sync.snapshots` -> `GitHistoryPanel`
- `sync.history` -> right sidebar CRDT history panel
- `sync-status`/`sync.status` -> `SyncStatusIndicator`

The web host keeps fallback UI when sync plugin loading/registration fails:
left sidebar still shows Share/Snapshots, right sidebar still shows History,
and the header still shows SyncStatusIndicator.

Both `LiveCollaborationPanel` and `PublishingPanel` read the selected audience from
`templateContextStore.previewAudience` (set by the `AudienceFilter` above the tabs)
instead of having their own audience dropdowns.

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
