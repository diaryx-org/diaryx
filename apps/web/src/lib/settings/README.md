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
  - '[ImportSettings.svelte](/apps/web/src/lib/settings/ImportSettings.svelte)'
  - '[LinkSettings.svelte](/apps/web/src/lib/settings/LinkSettings.svelte)'
  - '[S3BackupSettings.svelte](/apps/web/src/lib/settings/S3BackupSettings.svelte)'
  - '[StorageSettings.svelte](/apps/web/src/lib/settings/StorageSettings.svelte)'
  - '[SyncSettings.svelte](/apps/web/src/lib/settings/SyncSettings.svelte)'
  - '[syncSettingsLogic.ts](/apps/web/src/lib/settings/syncSettingsLogic.ts)'
  - '[TemplateSettings.svelte](/apps/web/src/lib/settings/TemplateSettings.svelte)'
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
| `ImportSettings.svelte` | Import from file |
| `LinkSettings.svelte` | Link format settings (workspace config) |
| `S3BackupSettings.svelte` | S3 backup configuration |
| `StorageSettings.svelte` | Storage backend settings |
| `SyncSettings.svelte` | Sync server settings + synced attachment storage usage |
| `syncSettingsLogic.ts` | Storage usage status/summary helpers for sync settings |
| `TemplateSettings.svelte` | Template management (default_template/daily_template in workspace config) |
| `WorkspaceSettings.svelte` | Workspace config: location, daily folder, entry behavior, filename style |

`SyncSettings.svelte` displays:

- sync connection/authentication state
- configured sync server URL
- a "Synced Storage" section with used bytes, blob count, and used/limit summary from `/api/user/storage`
- warning/over-limit states when nearing or exceeding per-user attachment quota

`ImportSettings.svelte` behavior:

- routes import through the sync server only for authenticated, sync-enabled,
  server-backed workspaces; local-only workspaces import locally even when signed in
- resolves the workspace directory from backend state (not only the current tree
  root), so replace imports can clear existing files even when the tree root is `.`
- ignores macOS metadata entries (such as `__MACOSX`) when stripping a common
  top-level ZIP folder, preventing nested-root imports
- emits `import:complete`; the app forces a full tree refresh after import so
  left-sidebar tree state updates immediately
