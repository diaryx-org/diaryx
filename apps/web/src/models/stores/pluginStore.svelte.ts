/**
 * Plugin Store - Manages plugin manifests and derived UI contributions.
 *
 * Fetches plugin manifests from the backend on init and provides
 * derived selectors for each UI extension point (settings tabs,
 * sidebar tabs, command palette items, toolbar buttons, status bar items).
 */

import type { Api } from '$lib/backend/api';
import type {
  PluginManifest,
  UiContribution,
  PluginId,
} from '$lib/backend/generated';
import { getBrowserManifests } from '$lib/plugins/browserPluginManager.svelte';

// ============================================================================
// State
// ============================================================================

/** Manifests from the native backend (Rust plugin registry). */
let backendManifests = $state<PluginManifest[]>([]);

/** Runtime manifest overrides (for plugins loaded outside backend registry). */
let runtimeManifestOverrides = $state<Record<string, PluginManifest>>({});

/** Combined manifests from backend + browser plugins + runtime overrides. */
const manifests = $derived.by(() => mergeManifests());

function mergeManifests(): PluginManifest[] {
  const byId = new Map<string, PluginManifest>();

  for (const manifest of backendManifests) {
    byId.set(String(manifest.id), manifest);
  }

  for (const manifest of getBrowserManifests()) {
    byId.set(String(manifest.id), manifest);
  }

  // Runtime overrides have highest precedence (e.g. sync Extism manifest in browser).
  for (const [pluginId, manifest] of Object.entries(runtimeManifestOverrides)) {
    byId.set(pluginId, manifest);
  }

  return Array.from(byId.values());
}

// ============================================================================
// Derived Selectors
// ============================================================================

/** All settings tab contributions across all plugins. */
function getSettingsTabs(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'SettingsTab' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter((c): c is Extract<UiContribution, { slot: 'SettingsTab' }> => c.slot === 'SettingsTab')
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

/** Left sidebar tab contributions. */
function getLeftSidebarTabs(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'SidebarTab' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: 'SidebarTab' }> =>
          c.slot === 'SidebarTab' && c.side === 'Left'
      )
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

/** Right sidebar tab contributions. */
function getRightSidebarTabs(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'SidebarTab' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: 'SidebarTab' }> =>
          c.slot === 'SidebarTab' && c.side === 'Right'
      )
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

/** Command palette item contributions. */
function getCommandPaletteItems(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'CommandPaletteItem' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: 'CommandPaletteItem' }> =>
          c.slot === 'CommandPaletteItem'
      )
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

/** Toolbar button contributions. */
function getToolbarButtons(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'ToolbarButton' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: 'ToolbarButton' }> =>
          c.slot === 'ToolbarButton'
      )
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

/** Status bar item contributions. */
function getStatusBarItems(): Array<{ pluginId: PluginId; contribution: Extract<UiContribution, { slot: 'StatusBarItem' }> }> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: 'StatusBarItem' }> =>
          c.slot === 'StatusBarItem'
      )
      .map((contribution) => ({ pluginId: m.id, contribution }))
  );
}

// ============================================================================
// Actions
// ============================================================================

/** Fetch plugin manifests from the backend. Call once during app init. */
async function init(api: Api): Promise<void> {
  try {
    backendManifests = await api.getPluginManifests();
  } catch (e) {
    console.warn('[pluginStore] Failed to load plugin manifests:', e);
    backendManifests = [];
  }
}

/** Register/update a runtime manifest override. */
function setRuntimeManifestOverride(manifest: PluginManifest): void {
  runtimeManifestOverrides = {
    ...runtimeManifestOverrides,
    [String(manifest.id)]: manifest,
  };
}

/** Remove a runtime manifest override by plugin id. */
function clearRuntimeManifestOverride(pluginId: PluginId | string): void {
  const id = String(pluginId);
  if (!(id in runtimeManifestOverrides)) return;
  const next = { ...runtimeManifestOverrides };
  delete next[id];
  runtimeManifestOverrides = next;
}

/** Clear all runtime manifest overrides. */
function clearRuntimeManifestOverrides(): void {
  runtimeManifestOverrides = {};
}

// ============================================================================
// Store Export
// ============================================================================

export function getPluginStore() {
  return {
    get manifests() { return manifests; },
    get settingsTabs() { return getSettingsTabs(); },
    get leftSidebarTabs() { return getLeftSidebarTabs(); },
    get rightSidebarTabs() { return getRightSidebarTabs(); },
    get commandPaletteItems() { return getCommandPaletteItems(); },
    get toolbarButtons() { return getToolbarButtons(); },
    get statusBarItems() { return getStatusBarItems(); },
    init,
    setRuntimeManifestOverride,
    clearRuntimeManifestOverride,
    clearRuntimeManifestOverrides,
  };
}
