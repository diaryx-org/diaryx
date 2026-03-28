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
  - "[ImportSettings.svelte](/apps/web/src/lib/settings/ImportSettings.svelte)"
  - "[LinkSettings.svelte](/apps/web/src/lib/settings/LinkSettings.svelte)"
  - "[StorageSettings.svelte](/apps/web/src/lib/settings/StorageSettings.svelte)"
  - "[WorkspaceManagement.svelte](/apps/web/src/lib/settings/WorkspaceManagement.svelte)"
  - "[WorkspaceSettings.svelte](/apps/web/src/lib/settings/WorkspaceSettings.svelte)"
  - "[PluginsSettings.svelte](/apps/web/src/lib/settings/PluginsSettings.svelte)"
  - "[PluginSettingsTab.svelte](/apps/web/src/lib/settings/PluginSettingsTab.svelte)"
  - "[ICloudSettings.svelte](/apps/web/src/lib/settings/ICloudSettings.svelte)"
  - "[syncSettingsLogic.ts](/apps/web/src/lib/settings/syncSettingsLogic.ts)"
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
| `DebugInfo.svelte` | Runtime app/config path diagnostics. On Tauri it can read the active log file into the panel and, on desktop, reveal that file in the system file manager. |
| `ImportSettings.svelte` | ZIP import flow for importing a Diaryx workspace export. |
| `AccountSettings.svelte` / `BillingSettings.svelte` | Authentication/account and billing surfaces. |
| `PluginsSettings.svelte` | Installed/local plugin management surface. Includes local `.wasm` upload, enable/disable, uninstall, and a shortcut into the dedicated marketplace. Registry installs are SHA-256 verified, and local installs review requested permissions on both browser and Tauri paths before install. Uninstall also clears workspace-level `plugins.<id>` / `disabled_plugins` entries through the Rust backend command path and drops plugin-owned local metadata / namespaces when present. |
| `PluginSettingsTab.svelte` | Declarative plugin field renderer, including generic host actions, follow-up commands, workspace metadata patch handling, and temporary file-byte bridging for plugin commands that call `host_request_file`. |
| `ICloudSettings.svelte` | iCloud Drive toggle and sync status (iOS Apple builds only). Conditionally rendered in the Data tab. |
| `syncSettingsLogic.ts` | Shared sync/storage usage helpers used by settings UIs. |

## Plugin Settings Tabs

`SettingsDialog.svelte` renders plugin-contributed settings tabs dynamically:

- `ComponentRef::Iframe` contributions render via `PluginIframe` (still used by sync snapshots/history and templating plugin panels)
- `ComponentRef::Builtin` contributions can resolve through `pluginBuiltinCompat` for host-backed compatibility fields when needed
- Declarative field contributions render via `PluginSettingsTab`
- `PluginSettingsTab` can invoke arbitrary host-managed actions, apply config patches, write plugin-scoped workspace metadata patches from command results, gate nested field groups with conditions like `authenticated` or `config:import_format=markdown`, and pass selected file bytes through both browser and native plugin command paths

Google Drive storage uses declarative settings plus a host-managed OAuth action. `diaryx.sync` and `diaryx.share` both use declarative settings surfaces, while snapshots/history and templating remain iframe-backed.

## Mobile Drawer Layout

`SettingsDialog.svelte` keeps mobile tab content scrollable by using a strict
flex-height chain (`h-[70vh]`, `min-h-0`, `flex-1`) inside `Drawer.Content`,
so long tabs (for example Workspace) do not push the bottom tab bar off-screen.

## ZIP Import Behavior

- `ImportSettings.svelte` uses backend ZIP import APIs for large backup imports.
- `diaryx.import` now owns the Day One / markdown format import settings tab declaratively, using generic host actions for file, directory, and workspace-entry picking.
- The shared ZIP import helpers still stream Markdown Directory ZIP imports with `@zip.js/zip.js` so large archives are processed entry-by-entry instead of loading the full ZIP into one `ArrayBuffer` first.

The shared settings content scroller is also reused by `WorkspaceManagement.svelte`
to preserve scroll position when inline workspace actions swap a row into
confirmation controls (for example, delete confirmations in the Account tab).

`DebugInfo.svelte` now reflects the backend-provided log path (`log_file`) and
can read the native log file directly through the Tauri backend. That lets the
Debug tab show the current log contents in-app while still offering a desktop
"Reveal Log File" action when the platform supports opening Finder/Explorer.
Plugin install failures on Tauri now emit stage-specific diagnostics into that
same log so mobile/TestFlight issues can be debugged without attaching Xcode.

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
