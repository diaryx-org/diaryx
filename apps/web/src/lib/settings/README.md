---
title: Settings
description: Settings panel components
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[index.ts](/apps/web/src/lib/settings/index.ts)"
  - "[AccountSettings.svelte](/apps/web/src/lib/settings/AccountSettings.svelte)"
  - "[BackupSettings.svelte](/apps/web/src/lib/settings/BackupSettings.svelte)"
  - "[BillingSettings.svelte](/apps/web/src/lib/settings/BillingSettings.svelte)"
  - "[ClearDataSettings.svelte](/apps/web/src/lib/settings/ClearDataSettings.svelte)"
  - "[DebugInfo.svelte](/apps/web/src/lib/settings/DebugInfo.svelte)"
  - "[DisplaySettings.svelte](/apps/web/src/lib/settings/DisplaySettings.svelte)"
  - "[FormatImportSettings.svelte](/apps/web/src/lib/settings/FormatImportSettings.svelte)"
  - "[ImportSettings.svelte](/apps/web/src/lib/settings/ImportSettings.svelte)"
  - "[LinkSettings.svelte](/apps/web/src/lib/settings/LinkSettings.svelte)"
  - "[S3StorageSettings.svelte](/apps/web/src/lib/settings/S3StorageSettings.svelte)"
  - "[GoogleDriveStorageSettings.svelte](/apps/web/src/lib/settings/GoogleDriveStorageSettings.svelte)"
  - "[StorageSettings.svelte](/apps/web/src/lib/settings/StorageSettings.svelte)"
  - "[SyncSettings.svelte](/apps/web/src/lib/settings/SyncSettings.svelte)"
  - "[syncActionStatusStore.svelte.ts](/apps/web/src/lib/settings/syncActionStatusStore.svelte.ts)"
  - "[syncSettingsLogic.ts](/apps/web/src/lib/settings/syncSettingsLogic.ts)"
  - "[workspaceSnapshotUpload.ts](/apps/web/src/lib/settings/workspaceSnapshotUpload.ts)"
  - "[TemplateSettings.svelte](/apps/web/src/lib/settings/TemplateSettings.svelte)"
  - "[WorkspaceManagement.svelte](/apps/web/src/lib/settings/WorkspaceManagement.svelte)"
  - "[WorkspaceSettings.svelte](/apps/web/src/lib/settings/WorkspaceSettings.svelte)"
  - "[AppearanceSettings.svelte](/apps/web/src/lib/settings/AppearanceSettings.svelte)"
  - "[PluginsSettings.svelte](/apps/web/src/lib/settings/PluginsSettings.svelte)"
  - "[ThemePresetCard.svelte](/apps/web/src/lib/settings/ThemePresetCard.svelte)"
  - "[AccentHuePicker.svelte](/apps/web/src/lib/settings/AccentHuePicker.svelte)"
exclude:
  - "*.lock"
---

# Settings

Settings panel components for the settings dialog.

## Files

| File                              | Purpose                                                                                        |
| --------------------------------- | ---------------------------------------------------------------------------------------------- |
| `AccountSettings.svelte`          | Account and login settings                                                                     |
| `BackupSettings.svelte`           | Local backup settings                                                                          |
| `BillingSettings.svelte`          | Subscription and billing settings (Stripe + Apple IAP)                                         |
| `ClearDataSettings.svelte`        | Clear data controls                                                                            |
| `DebugInfo.svelte`                | Debug information display                                                                      |
| `DisplaySettings.svelte`          | Display preferences (theme + focus mode)                                                       |
| `FormatImportSettings.svelte`     | Import from Day One or Markdown formats (uses WASM parsers + ImportEntries command)            |
| `ImportSettings.svelte`           | Import from file (raw ZIP extraction)                                                          |
| `LinkSettings.svelte`             | Bulk conversion for `part_of`/`contents`/`attachments` using the current workspace link format |
| `S3StorageSettings.svelte`        | S3-compatible storage plugin settings (Extism)                                                 |
| `GoogleDriveStorageSettings.svelte` | Google Drive storage plugin settings (Extism)                                                |
| `StorageSettings.svelte`          | Storage backend settings                                                                       |
| `SyncSettings.svelte`             | Sync server settings + synced attachment storage usage                                         |
| `syncActionStatusStore.svelte.ts` | Shared progress/status state for settings-driven sync actions                                  |
| `syncSettingsLogic.ts`            | Storage usage status/summary helpers for sync settings                                         |
| `workspaceSnapshotUpload.ts`      | Shared local-workspace snapshot builder used by sync bootstrap flows                           |
| `TemplateSettings.svelte`         | Template management (default_template/daily_template in workspace config)                      |
| `WorkspaceManagement.svelte`      | Local/synced workspace management and sync toggles                                             |
| `WorkspaceSettings.svelte`        | Workspace config: location and daily folder                                                    |
| `AppearanceSettings.svelte`       | Theme presets, accent hue, typography, and layout settings                                     |
| `PluginsSettings.svelte`          | Per-plugin enable/disable toggles                                                              |
| `ThemePresetCard.svelte`          | Preset preview card with color swatches                                                        |
| `AccentHuePicker.svelte`          | Hue range slider (0–360) with conic gradient                                                   |

`PluginsSettings.svelte` behavior:

- lists all known plugin manifests (backend, browser-loaded, and runtime overrides)
- stores enable/disable state per plugin in localStorage
- disables plugin UI contributions (settings tabs, sidebar tabs, command palette items, toolbar/status items) when toggled off
- blocks browser plugin event/command dispatch when a plugin is disabled
- shows a runtime support notice and disables plugin toggles when browser Extism plugins are unsupported (for example, WebKit without JSPI)

`SyncSettings.svelte` displays:

- sync connection/authentication state
- configured sync server URL
- a "Synced Storage" section with used bytes, blob count, and used/limit summary from `/api/user/storage`
- warning/over-limit states when nearing or exceeding per-user attachment quota
- in-progress feedback for settings-triggered workspace sync actions (stage text + progress bar)

`WorkspaceManagement.svelte` behavior:

- provider-agnostic: uses `workspaceProviderService` for link/unlink operations
- "Link to provider" button uses the first available `WorkspaceProvider` from
  `pluginStore.workspaceProviders` (falls back to hidden when no provider enabled)
- "Unlink" disconnects live sync and marks workspace as local-only
- synced workspace classification is derived from `getServerWorkspaceId()` presence
- publishes staged progress to the shared sync status bar via `syncActionStatusStore`

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

`AddWorkspaceDialog.svelte` behavior:

- local-first: workspaces are always created locally first
- optional provider dropdown ("None / local only" default) to link via
  `workspaceProviderService.linkWorkspace()` after local creation
- three content sources: Start fresh, Import ZIP, Open folder
- removed: auth/upgrade screens, existing_workspace content source, sync mode
  toggle (replaced by provider dropdown)

`BillingSettings.svelte` behavior:

- avoids eager Apple IAP product fetch on component mount so opening the
  settings dialog does not invoke StoreKit automatically (important for
  simulator/dev builds)
