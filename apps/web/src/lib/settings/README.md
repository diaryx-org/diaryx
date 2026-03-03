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
  - "[StorageSettings.svelte](/apps/web/src/lib/settings/StorageSettings.svelte)"
  - "[WorkspaceManagement.svelte](/apps/web/src/lib/settings/WorkspaceManagement.svelte)"
  - "[WorkspaceSettings.svelte](/apps/web/src/lib/settings/WorkspaceSettings.svelte)"
  - "[AppearanceSettings.svelte](/apps/web/src/lib/settings/AppearanceSettings.svelte)"
  - "[PluginsSettings.svelte](/apps/web/src/lib/settings/PluginsSettings.svelte)"
  - "[PluginSettingsTab.svelte](/apps/web/src/lib/settings/PluginSettingsTab.svelte)"
  - "[syncSettingsLogic.ts](/apps/web/src/lib/settings/syncSettingsLogic.ts)"
  - "[workspaceSnapshotUpload.ts](/apps/web/src/lib/settings/workspaceSnapshotUpload.ts)"
exclude:
  - "*.lock"
---

# Settings

Settings panel components for `SettingsDialog.svelte`.

## Files

| File | Purpose |
| --- | --- |
| `DisplaySettings.svelte` / `AppearanceSettings.svelte` | Display, typography, and theme preferences. |
| `WorkspaceSettings.svelte` / `WorkspaceManagement.svelte` | Workspace config and provider link/unlink management. |
| `StorageSettings.svelte` | Local storage backend settings. |
| `ImportSettings.svelte` / `FormatImportSettings.svelte` | ZIP import and format import flows. |
| `AccountSettings.svelte` / `BillingSettings.svelte` | Authentication/account and billing surfaces. |
| `PluginsSettings.svelte` | Plugin enable/disable + install/remove controls, including install-time permission review and default-permission persistence to root frontmatter. |
| `PluginSettingsTab.svelte` | Declarative plugin field renderer. |
| `syncSettingsLogic.ts` | Shared sync/storage usage helpers used by settings UIs. |
| `workspaceSnapshotUpload.ts` | Shared snapshot builder used by provider/sync bootstrap flows. |

## Plugin Settings Tabs

`SettingsDialog.svelte` renders plugin-contributed settings tabs dynamically:

- `ComponentRef::Iframe` contributions render via `PluginIframe` (used by sync, GDrive, templating plugins)
- Declarative field contributions render via `PluginSettingsTab`

All plugin settings (sync, GDrive storage, templating) now use the iframe approach.

## Managed AI Notes

- `SettingsDialog.svelte` now renders `UpgradeBanner` for `diaryx.ai` when:
  - `provider_mode === "managed"` and
  - current auth tier is not Plus.
- The AI provider mode selector remains visible so users can switch back to BYO mode.
- When `diaryx.ai` config is saved in managed mode, settings persistence ensures root frontmatter plugin permissions include the current sync server hostname under:
  - `plugins.diaryx.ai.permissions.http_requests.include`
  - no wildcard `all` is used.
