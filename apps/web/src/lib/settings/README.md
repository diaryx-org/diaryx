---
title: Settings
description: Settings panel components
part_of: '[README](/apps/web/src/lib/README.md)'
attachments:
  - '[index.ts](/apps/web/src/lib/settings/index.ts)'
  - '[AccountSettings.svelte](/apps/web/src/lib/settings/AccountSettings.svelte)'
  - '[BackupSettings.svelte](/apps/web/src/lib/settings/BackupSettings.svelte)'
  - '[ClearDataSettings.svelte](/apps/web/src/lib/settings/ClearDataSettings.svelte)'
  - '[CloudBackupSettings.svelte](/apps/web/src/lib/settings/CloudBackupSettings.svelte)'
  - '[DebugInfo.svelte](/apps/web/src/lib/settings/DebugInfo.svelte)'
  - '[DisplaySettings.svelte](/apps/web/src/lib/settings/DisplaySettings.svelte)'
  - '[FormattingSettings.svelte](/apps/web/src/lib/settings/FormattingSettings.svelte)'
  - '[GoogleDriveSettings.svelte](/apps/web/src/lib/settings/GoogleDriveSettings.svelte)'
  - '[FormatImportSettings.svelte](/apps/web/src/lib/settings/FormatImportSettings.svelte)'
  - '[ImportSettings.svelte](/apps/web/src/lib/settings/ImportSettings.svelte)'
  - '[LinkSettings.svelte](/apps/web/src/lib/settings/LinkSettings.svelte)'
  - '[S3BackupSettings.svelte](/apps/web/src/lib/settings/S3BackupSettings.svelte)'
  - '[StorageSettings.svelte](/apps/web/src/lib/settings/StorageSettings.svelte)'
  - '[SyncSettings.svelte](/apps/web/src/lib/settings/SyncSettings.svelte)'
  - '[syncActionStatusStore.svelte.ts](/apps/web/src/lib/settings/syncActionStatusStore.svelte.ts)'
  - '[syncSettingsLogic.ts](/apps/web/src/lib/settings/syncSettingsLogic.ts)'
  - '[workspaceSnapshotUpload.ts](/apps/web/src/lib/settings/workspaceSnapshotUpload.ts)'
  - '[TemplateSettings.svelte](/apps/web/src/lib/settings/TemplateSettings.svelte)'
  - '[WorkspaceManagement.svelte](/apps/web/src/lib/settings/WorkspaceManagement.svelte)'
  - '[WorkspaceSettings.svelte](/apps/web/src/lib/settings/WorkspaceSettings.svelte)'
exclude:
  - '*.lock'
---

# Settings

Settings panel components for the settings dialog.

## Files

| File | Purpose |
|------|---------|
| `AccountSettings.svelte` | Account and login settings |
| `BackupSettings.svelte` | Local backup settings |
| `ClearDataSettings.svelte` | Clear data controls |
| `CloudBackupSettings.svelte` | Cloud backup configuration |
| `DebugInfo.svelte` | Debug information display |
| `DisplaySettings.svelte` | Display preferences |
| `FormattingSettings.svelte` | Text formatting options |
| `GoogleDriveSettings.svelte` | Google Drive integration |
| `FormatImportSettings.svelte` | Import from Day One or Markdown formats (uses WASM parsers + ImportEntries command) |
| `ImportSettings.svelte` | Import from file (raw ZIP extraction) |
| `LinkSettings.svelte` | Link format settings (workspace config) |
| `S3BackupSettings.svelte` | S3 backup configuration |
| `StorageSettings.svelte` | Storage backend settings |
| `SyncSettings.svelte` | Sync server settings + synced attachment storage usage |
| `syncActionStatusStore.svelte.ts` | Shared progress/status state for settings-driven sync actions |
| `syncSettingsLogic.ts` | Storage usage status/summary helpers for sync settings |
| `workspaceSnapshotUpload.ts` | Shared local-workspace snapshot builder used by sync bootstrap flows |
| `TemplateSettings.svelte` | Template management (default_template/daily_template in workspace config) |
| `WorkspaceManagement.svelte` | Local/synced workspace management and sync toggles |
| `WorkspaceSettings.svelte` | Workspace config: location, daily folder, entry behavior, filename style |

`SyncSettings.svelte` displays:

- sync connection/authentication state
- configured sync server URL
- a "Synced Storage" section with used bytes, blob count, and used/limit summary from `/api/user/storage`
- warning/over-limit states when nearing or exceeding per-user attachment quota
- in-progress feedback for settings-triggered workspace sync actions (stage text + progress bar)

`WorkspaceManagement.svelte` behavior:

- stopping sync on the currently open workspace now also disconnects the live
  sync transport and clears the active sync workspace ID, preventing local-only
  edits/imports from continuing to upload
- local workspace rows now surface when a cloud copy still exists and provide a
  direct "delete cloud copy" action, so localized workspaces can reclaim server
  attachment storage without re-enabling sync
- starting sync for a local workspace first attempts to relink an existing cloud
  workspace by ID/name before creating a new server workspace, avoiding false
  name-conflict failures when sync was previously stopped or server state was stale
- for current-workspace local→cloud migration, seeds newly created cloud
  workspaces with a snapshot upload before enabling sync to avoid expensive
  regular CRDT bootstrap in the browser
- publishes staged + byte-level upload progress to the shared sync status bar
  so users can see migration progress from both Account and Sync tabs

`ImportSettings.svelte` behavior:

- routes import through the sync server only for authenticated, sync-enabled,
  server-backed workspaces; local-only workspaces import locally even when signed in
- resolves the workspace directory from backend state (not only the current tree
  root), so replace imports can clear existing files even when the tree root is `.`
- ignores macOS metadata entries (such as `__MACOSX`) when stripping a common
  top-level ZIP folder, preventing nested-root imports
- performs local ZIP extraction with a streaming reader to reduce peak memory
  usage on large imports
- forces local-only workspaces to disconnect stale sync state before local import
  begins, so large local imports cannot accidentally stream CRDT changes to cloud
- shows import progress with a percent + progress bar (callback-driven for local
  imports and staged + upload-byte progress for sync-server imports)
- emits `import:complete`; the app forces a full tree refresh after import so
  left-sidebar tree state updates immediately

`AddWorkspaceDialog.svelte` ZIP-import behavior:

- uploads the selected ZIP to the server with byte-level progress and explicit
  "server is importing" feedback once upload bytes are complete
- applies the same ZIP locally (instead of re-downloading a snapshot) before
  CRDT initialization, reducing a large network roundtrip during setup
