---
title: Settings
description: Settings panel components
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
  - "**/*.ts"
---

# Settings

Settings panel components for `SettingsDialog.svelte`.

## Files

| File | Purpose |
| --- | --- |
| `DisplaySettings.svelte` | Display mode and focus-mode preferences. |
| `WorkspaceSettings.svelte` / `WorkspaceManagement.svelte` | Workspace config and read-only local workspace overview. |
| `StorageSettings.svelte` | Local storage backend settings. Plugin/cloud storage providers are not shown as workspace sync choices. |
| `DebugInfo.svelte` | Runtime app/config path diagnostics. On Tauri it can read the active log file into the panel and, on desktop, reveal that file in the system file manager. |
| `ImportSettings.svelte` | ZIP import flow for importing a Diaryx workspace export. |
| `AccountSettings.svelte` / `BillingSettings.svelte` | Authentication/account and billing surfaces. |
| `PluginsSettings.svelte` | Installed/local plugin management surface. Includes local `.wasm` upload, enable/disable, uninstall, and a shortcut into the dedicated marketplace. Registry installs are SHA-256 verified, and local installs review requested permissions on both browser and Tauri paths before install. Uninstall also clears workspace-level `plugins.<id>` / `disabled_plugins` entries through the Rust backend command path and drops plugin-owned local metadata / namespaces when present. |
| `PluginSettingsTab.svelte` | Declarative plugin field renderer, including generic host actions, follow-up commands, workspace metadata patch handling, and temporary file-byte bridging for plugin commands that call `host_request_file`. |
| `SyncLinkSettings.svelte` / `ICloudSettings.svelte` | Legacy sync settings components retained while the old provider-sync implementation is unwound; not rendered by `SettingsDialog.svelte`. |
| `syncSettingsLogic.ts` | Legacy sync/storage usage helpers used by legacy settings UIs. |

`WorkspaceManagement.svelte` now shows only local workspaces and explains that
users can place the folder in an external sync tool such as iCloud Drive,
Dropbox, Syncthing, or Git. It no longer lists cloud-linked, downloadable, or
provider-unavailable workspaces.

## Plugin Settings Tabs

`SettingsDialog.svelte` renders plugin-contributed settings tabs dynamically:

- `ComponentRef::Iframe` contributions render via `PluginIframe` (used by plugin panels such as templating)
- `ComponentRef::Builtin` contributions can resolve through `pluginBuiltinCompat` for host-backed compatibility fields when needed
- Declarative field contributions render via `PluginSettingsTab`
- `PluginSettingsTab` can invoke arbitrary host-managed actions, apply config patches, write plugin-scoped workspace metadata patches from command results, gate nested field groups with conditions like `authenticated` or `config:import_format=markdown`, and pass selected file bytes through both browser and native plugin command paths

Declarative plugin settings remain available for non-sync plugins and
host-managed actions such as OAuth. Legacy sync provider settings should stay
out of the active settings tabs unless the product direction changes again.

## Mobile Drawer Layout

`SettingsDialog.svelte` keeps mobile tab content scrollable by using a strict
flex-height chain (`h-[70vh]`, `min-h-0`, `flex-1`) inside `Drawer.Content`,
so long tabs (for example Workspace) do not push the bottom tab bar off-screen.

## ZIP Import Behavior

- `ImportSettings.svelte` uses backend ZIP import APIs for large backup imports.
- `diaryx.import` now owns the Day One / markdown format import settings tab declaratively, using generic host actions for file, directory, and workspace-entry picking.
- The shared ZIP import helpers still stream Markdown Directory ZIP imports with `@zip.js/zip.js` so large archives are processed entry-by-entry instead of loading the full ZIP into one `ArrayBuffer` first.

The shared settings content scroller is reused across tabs so long sections in
the Account, Data, and plugin tabs keep a stable scroll container while content
changes within the active tab. Workspace deletion confirmations now live in
`WorkspaceSelector.svelte`, not in the Account tab's read-only workspace list.

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
- When `diaryx.ai` config is saved in managed mode, settings persistence ensures root frontmatter plugin permissions include the current backend hostname under:
  - `plugins.diaryx.ai.permissions.http_requests.include`
  - no wildcard `all` is used.
