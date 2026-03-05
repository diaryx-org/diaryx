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
| `DisplaySettings.svelte` | Display mode and focus-mode preferences. |
| `WorkspaceSettings.svelte` / `WorkspaceManagement.svelte` | Workspace config and provider link/unlink management. |
| `StorageSettings.svelte` | Local storage backend settings. |
| `ImportSettings.svelte` / `FormatImportSettings.svelte` | ZIP import and format import flows. |
| `AccountSettings.svelte` / `BillingSettings.svelte` | Authentication/account and billing surfaces. |
| `PluginsSettings.svelte` | Installed/local plugin management surface. Includes local `.wasm` upload, enable/disable, uninstall, and a shortcut into the dedicated marketplace. Registry installs are SHA-256 verified. |
| `PluginSettingsTab.svelte` | Declarative plugin field renderer. |
| `syncSettingsLogic.ts` | Shared sync/storage usage helpers used by settings UIs. |
| `workspaceSnapshotUpload.ts` | Shared snapshot builder used by provider/sync bootstrap flows. |

## Plugin Settings Tabs

`SettingsDialog.svelte` renders plugin-contributed settings tabs dynamically:

- `ComponentRef::Iframe` contributions render via `PluginIframe` (used by sync, GDrive, templating plugins)
- Declarative field contributions render via `PluginSettingsTab`

All plugin settings (sync, GDrive storage, templating) now use the iframe approach.

## Mobile Drawer Layout

`SettingsDialog.svelte` keeps mobile tab content scrollable by using a strict
flex-height chain (`h-[70vh]`, `min-h-0`, `flex-1`) inside `Drawer.Content`,
so long tabs (for example Workspace) do not push the bottom tab bar off-screen.

## Marketplace Integration

- The dedicated marketplace is a separate app surface (`views/marketplace/PluginMarketplace.svelte`), not just a settings panel.
- Settings keeps an installed/local management focus and links to marketplace via `Open Marketplace`.
- Theme and typography customization now lives in the marketplace tabs (`Themes`, `Typography`, `Bundles`) rather than a separate settings section, including installable typography presets with per-field overrides.
- Local uploads are explicitly labeled unmanaged and are separate from curated registry trust.

## Managed AI Notes

- `SettingsDialog.svelte` now renders `UpgradeBanner` for `diaryx.ai` when:
  - `provider_mode === "managed"` and
  - current auth tier is not Plus.
- The AI provider mode selector remains visible so users can switch back to BYO mode.
- When `diaryx.ai` config is saved in managed mode, settings persistence ensures root frontmatter plugin permissions include the current sync server hostname under:
  - `plugins.diaryx.ai.permissions.http_requests.include`
  - no wildcard `all` is used.
