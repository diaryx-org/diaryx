/**
 * Plugin Store - Manages plugin manifests and derived UI contributions.
 *
 * Fetches plugin manifests from the backend on init and provides
 * derived selectors for each UI extension point (settings tabs,
 * sidebar tabs, command palette items, toolbar buttons, status bar items).
 */

import type { Api } from "$lib/backend/api";
import type {
  PluginManifest,
  UiContribution,
  PluginId,
} from "$lib/backend/generated";
import type { Component } from "svelte";
import { getBrowserManifests } from "$lib/plugins/browserPluginManager.svelte";
import {
  loadPluginIcon,
  getCachedPluginIcon,
} from "$lib/plugins/pluginIconResolver";

/** A plugin insert command ready for rendering in editor menus. */
export interface PluginInsertCommand {
  /** The TipTap node name (extension_id). */
  extensionId: string;
  /** Display label for the button. */
  label: string;
  /** Lucide icon name (kebab-case). */
  iconName: string | null;
  /** Resolved Svelte icon component (or fallback). */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  icon: Component<any>;
  /** Tooltip text. */
  description: string | null;
  /** Whether this is inline or block. */
  nodeType: "InlineAtom" | "BlockAtom";
}

const PLUGIN_ENABLED_KEY = "diaryx-plugin-enabled";

// ============================================================================
// State
// ============================================================================

/** Manifests from the native backend (Rust plugin registry). */
let backendManifests = $state<PluginManifest[]>([]);

/** Runtime manifest overrides (for plugins loaded outside backend registry). */
let runtimeManifestOverrides = $state<Record<string, PluginManifest>>({});

/** Persisted enable/disable state by plugin ID. Defaults to true. */
let pluginEnabledState = $state<Record<string, boolean>>({});

/** Combined manifests from backend + browser plugins + runtime overrides. */
const allManifests = $derived.by(() => mergeManifests());

/** Enabled manifests used for plugin contributions. */
const manifests = $derived.by(() =>
  allManifests.filter((manifest) => isPluginEnabled(String(manifest.id))),
);

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

function loadPluginEnabledState(): Record<string, boolean> {
  if (typeof localStorage === "undefined") {
    return {};
  }

  try {
    const raw = localStorage.getItem(PLUGIN_ENABLED_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Record<string, boolean>;
  } catch {
    return {};
  }
}

function persistPluginEnabledState(next: Record<string, boolean>): void {
  if (typeof localStorage === "undefined") {
    return;
  }

  try {
    localStorage.setItem(PLUGIN_ENABLED_KEY, JSON.stringify(next));
  } catch {
    // Ignore storage write errors.
  }
}

function isPluginEnabled(pluginId: PluginId | string): boolean {
  const id = String(pluginId);
  const value = pluginEnabledState[id];
  return value !== false;
}

function setPluginEnabled(pluginId: PluginId | string, enabled: boolean): void {
  const id = String(pluginId);
  const current = pluginEnabledState[id];

  if ((current ?? true) === enabled) {
    return;
  }

  const next = {
    ...pluginEnabledState,
    [id]: enabled,
  };

  pluginEnabledState = next;
  persistPluginEnabledState(next);
}

// ============================================================================
// Derived Selectors
// ============================================================================

/** All settings tab contributions across all plugins. */
function getSettingsTabs(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "SettingsTab" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "SettingsTab" }> =>
          c.slot === "SettingsTab",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Left sidebar tab contributions. */
function getLeftSidebarTabs(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "SidebarTab" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "SidebarTab" }> =>
          c.slot === "SidebarTab" && c.side === "Left",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Right sidebar tab contributions. */
function getRightSidebarTabs(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "SidebarTab" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "SidebarTab" }> =>
          c.slot === "SidebarTab" && c.side === "Right",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Command palette item contributions. */
function getCommandPaletteItems(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "CommandPaletteItem" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "CommandPaletteItem" }> =>
          c.slot === "CommandPaletteItem",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Plugin-owned command palette surface (at most one, first match wins). */
function getCommandPaletteOwner(): {
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "CommandPalette" }>;
} | null {
  for (const manifest of manifests) {
    const match = manifest.ui.find(
      (c): c is Extract<UiContribution, { slot: "CommandPalette" }> =>
        c.slot === "CommandPalette",
    );
    if (match) {
      return { pluginId: manifest.id, contribution: match };
    }
  }
  return null;
}

/** Plugin-owned left sidebar tree context menu surface (first match wins). */
function getLeftSidebarContextMenuOwner(): {
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "ContextMenu" }>;
} | null {
  for (const manifest of manifests) {
    const match = manifest.ui.find(
      (c): c is Extract<UiContribution, { slot: "ContextMenu" }> =>
        c.slot === "ContextMenu" && c.target === "LeftSidebarTree",
    );
    if (match) {
      return { pluginId: manifest.id, contribution: match };
    }
  }
  return null;
}

/** Toolbar button contributions. */
function getToolbarButtons(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "ToolbarButton" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "ToolbarButton" }> =>
          c.slot === "ToolbarButton",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Status bar item contributions. */
function getStatusBarItems(): Array<{
  pluginId: PluginId;
  contribution: Extract<UiContribution, { slot: "StatusBarItem" }>;
}> {
  return manifests.flatMap((m) =>
    m.ui
      .filter(
        (c): c is Extract<UiContribution, { slot: "StatusBarItem" }> =>
          c.slot === "StatusBarItem",
      )
      .map((contribution) => ({ pluginId: m.id, contribution })),
  );
}

/** Editor insert commands from EditorExtension entries with insert_command. */
function getEditorInsertCommands(): {
  inline: PluginInsertCommand[];
  block: PluginInsertCommand[];
} {
  const inline: PluginInsertCommand[] = [];
  const block: PluginInsertCommand[] = [];

  for (const manifest of manifests) {
    for (const ui of manifest.ui) {
      if (ui.slot !== "EditorExtension") continue;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const ext = ui as any;
      if (!ext.insert_command) continue;

      const iconName: string | null = ext.insert_command.icon ?? null;
      const cmd: PluginInsertCommand = {
        extensionId: ext.extension_id,
        label: ext.insert_command.label,
        iconName,
        icon: getCachedPluginIcon(iconName),
        description: ext.insert_command.description ?? null,
        nodeType: ext.node_type,
      };

      if (ext.node_type === "InlineAtom") {
        inline.push(cmd);
      } else {
        block.push(cmd);
      }
    }
  }

  return { inline, block };
}

/** Eagerly load icons for all plugin insert commands. Call after plugins load. */
async function preloadInsertCommandIcons(): Promise<void> {
  for (const manifest of manifests) {
    for (const ui of manifest.ui) {
      if (ui.slot !== "EditorExtension") continue;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const ext = ui as any;
      if (ext.insert_command?.icon) {
        await loadPluginIcon(ext.insert_command.icon);
      }
    }
  }
}

// ============================================================================
// Actions
// ============================================================================

/** Fetch plugin manifests from the backend. Call once during app init. */
async function init(api: Api): Promise<void> {
  pluginEnabledState = loadPluginEnabledState();

  try {
    backendManifests = await api.getPluginManifests();
  } catch (e) {
    console.warn("[pluginStore] Failed to load plugin manifests:", e);
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
    get allManifests() {
      return allManifests;
    },
    get manifests() {
      return manifests;
    },
    get settingsTabs() {
      return getSettingsTabs();
    },
    get leftSidebarTabs() {
      return getLeftSidebarTabs();
    },
    get rightSidebarTabs() {
      return getRightSidebarTabs();
    },
    get commandPaletteItems() {
      return getCommandPaletteItems();
    },
    get commandPaletteOwner() {
      return getCommandPaletteOwner();
    },
    get leftSidebarContextMenuOwner() {
      return getLeftSidebarContextMenuOwner();
    },
    get toolbarButtons() {
      return getToolbarButtons();
    },
    get statusBarItems() {
      return getStatusBarItems();
    },
    get editorInsertCommands() {
      return getEditorInsertCommands();
    },
    preloadInsertCommandIcons,
    isPluginEnabled,
    setPluginEnabled,
    init,
    setRuntimeManifestOverride,
    clearRuntimeManifestOverride,
    clearRuntimeManifestOverrides,
  };
}
