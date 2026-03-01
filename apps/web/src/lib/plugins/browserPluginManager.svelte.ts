/**
 * Browser Plugin Manager — manages the lifecycle of browser-loaded Extism plugins.
 *
 * Plugins are stored as .wasm blobs in IndexedDB and loaded at startup.
 * Their manifests are merged into the pluginStore alongside any manifests
 * from the native backend.
 */

import {
  loadBrowserPlugin,
  getBrowserPluginRuntimeSupport,
  type BrowserExtismPlugin,
  type GuestEvent,
} from "./extismBrowserLoader";
import type { PluginManifest } from "$lib/backend/generated";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import {
  createExtensionFromManifest,
  isEditorExtension,
  type EditorExtensionManifest,
} from "./editorExtensionFactory";

// ============================================================================
// Built-in plugin constants
// ============================================================================

/** IDs of plugins that ship with the app and are auto-installed at startup. */
export const BUILTIN_PLUGIN_IDS = new Set([
  "diaryx.ai",
  "diaryx.math",
  "publish",
]);

/** Built-in plugin metadata for auto-install (fallback URLs in public/plugins/). */
export const BUILTIN_PLUGINS = [
  { url: "/plugins/diaryx_ai.wasm", id: "diaryx.ai", name: "AI Assistant" },
  { url: "/plugins/diaryx_math.wasm", id: "diaryx.math", name: "Math" },
  { url: "/plugins/diaryx_publish.wasm", id: "publish", name: "Publish" },
];

/** Check whether a plugin ID is a built-in plugin. */
export function isBuiltinPlugin(pluginId: string): boolean {
  return BUILTIN_PLUGIN_IDS.has(pluginId);
}

// ============================================================================
// IndexedDB storage
// ============================================================================

const DB_NAME = "diaryx-plugins";
const DB_VERSION = 1;
const STORE_NAME = "plugins";

interface StoredPlugin {
  /** Plugin ID (primary key). */
  id: string;
  /** Display name. */
  name: string;
  /** Raw WASM bytes. */
  wasm: ArrayBuffer;
  /** Timestamp of installation. */
  installedAt: number;
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: "id" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function getAllStoredPlugins(): Promise<StoredPlugin[]> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const request = store.getAll();
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function storePlugin(entry: StoredPlugin): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.put(entry);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
}

async function removeStoredPlugin(id: string): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const request = store.delete(id);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
}

// ============================================================================
// Manager state
// ============================================================================

/** Map of loaded plugin instances by ID. */
const loadedPlugins = new Map<string, BrowserExtismPlugin>();

/** Reactive array of browser plugin manifests. */
let browserManifests = $state<PluginManifest[]>([]);

/** Last runtime-level support error for browser plugins. */
let runtimeSupportError = $state<string | null>(null);

// ============================================================================
// Public API
// ============================================================================

/**
 * Install a plugin from raw WASM bytes.
 *
 * The bytes are persisted in IndexedDB and the plugin is loaded immediately.
 * Returns the installed plugin's manifest.
 */
export async function installPlugin(
  wasmBytes: ArrayBuffer,
  name?: string,
): Promise<PluginManifest> {
  const support = getBrowserPluginRuntimeSupport();
  if (!support.supported) {
    runtimeSupportError =
      support.reason ?? "Browser plugins are not supported in this runtime.";
    throw new Error(runtimeSupportError);
  }

  console.log(
    `[browserPluginManager] Installing plugin (${(wasmBytes.byteLength / 1024).toFixed(0)} KB)...`,
  );
  const plugin = await loadBrowserPlugin(wasmBytes);
  const id = plugin.manifest.id as unknown as string;
  console.log(
    `[browserPluginManager] Installed plugin: ${id} (${plugin.manifest.name})`,
  );

  // Close and remove any existing instance with the same ID (idempotent reinstall).
  const existing = loadedPlugins.get(id);
  if (existing) {
    await existing.close();
    loadedPlugins.delete(id);
    browserManifests = browserManifests.filter(
      (m) => (m.id as unknown as string) !== id,
    );
  }

  // Persist to IndexedDB.
  await storePlugin({
    id,
    name: name ?? plugin.manifest.name,
    wasm: wasmBytes,
    installedAt: Date.now(),
  });

  // Track loaded instance.
  loadedPlugins.set(id, plugin);
  browserManifests = [...browserManifests, plugin.manifest];

  return plugin.manifest;
}

/**
 * Uninstall a plugin by ID.
 *
 * Closes the plugin instance and removes it from IndexedDB.
 */
export async function uninstallPlugin(pluginId: string): Promise<void> {
  const plugin = loadedPlugins.get(pluginId);
  if (plugin) {
    await plugin.close();
    loadedPlugins.delete(pluginId);
  }
  await removeStoredPlugin(pluginId);
  browserManifests = browserManifests.filter(
    (m) => (m.id as unknown as string) !== pluginId,
  );
  getPluginStore().clearPluginEnabled(pluginId);
}

/**
 * Load all installed plugins from IndexedDB.
 *
 * Called once at app startup. Plugins that fail to load are logged and skipped.
 */
export async function loadAllPlugins(): Promise<void> {
  const support = getBrowserPluginRuntimeSupport();
  if (!support.supported) {
    runtimeSupportError =
      support.reason ?? "Browser plugins are not supported in this runtime.";
    console.info("[browserPluginManager]", runtimeSupportError);
    return;
  }

  runtimeSupportError = null;

  try {
    const stored = await getAllStoredPlugins();
    console.log(
      `[browserPluginManager] Found ${stored.length} stored plugin(s)`,
    );
    for (const entry of stored) {
      try {
        const plugin = await loadBrowserPlugin(entry.wasm);
        const id = plugin.manifest.id as unknown as string;
        loadedPlugins.set(id, plugin);
        browserManifests = [...browserManifests, plugin.manifest];
        console.log(
          `[browserPluginManager] Loaded plugin: ${id} (${plugin.manifest.name})`,
        );
      } catch (e) {
        console.warn(
          `[browserPluginManager] Failed to load plugin ${entry.id}:`,
          e,
        );
      }
    }
  } catch (e) {
    console.warn(
      "[browserPluginManager] Failed to load plugins from IndexedDB:",
      e,
    );
  }
}

/**
 * Get a loaded plugin instance by ID.
 */
export function getPlugin(pluginId: string): BrowserExtismPlugin | undefined {
  return loadedPlugins.get(pluginId);
}

/**
 * Get the current list of browser-loaded plugin manifests (reactive).
 */
export function getBrowserManifests(): PluginManifest[] {
  return browserManifests;
}

/** Runtime support for browser-loaded plugins in the current browser. */
export function getBrowserPluginSupport(): {
  supported: boolean;
  reason?: string;
} {
  return getBrowserPluginRuntimeSupport();
}

/** Last runtime support error encountered by plugin manager. */
export function getBrowserPluginSupportError(): string | null {
  return runtimeSupportError;
}

// ============================================================================
// Editor extension generation
// ============================================================================

/** Cache of generated TipTap extensions (rebuilt when plugins change). */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let cachedEditorExtensions: any[] | null = null;
let cachedPluginCount = 0;

/**
 * Get TipTap extensions generated from all loaded plugins' EditorExtension manifests.
 *
 * Returns an array of TipTap Node extensions ready to register with the editor.
 * Results are cached and invalidated when plugin count changes.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function getEditorExtensions(): any[] {
  if (cachedEditorExtensions && cachedPluginCount === loadedPlugins.size) {
    return cachedEditorExtensions;
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const extensions: any[] = [];

  for (const [, plugin] of loadedPlugins) {
    const manifest = plugin.manifest;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const uiEntries = (manifest.ui ?? []) as any[];
    for (const ui of uiEntries) {
      if (isEditorExtension(ui)) {
        try {
          const ext = createExtensionFromManifest(
            ui as EditorExtensionManifest,
            plugin,
          );
          extensions.push(ext);
        } catch (e) {
          console.warn(
            `[browserPluginManager] Failed to create editor extension ${ui.extension_id}:`,
            e,
          );
        }
      }
    }
  }

  cachedEditorExtensions = extensions;
  cachedPluginCount = loadedPlugins.size;
  return extensions;
}

// ============================================================================
// Event dispatch helpers
// ============================================================================

/**
 * Dispatch a lifecycle event to all loaded browser plugins.
 */
export async function dispatchEvent(event: GuestEvent): Promise<void> {
  const pluginStore = getPluginStore();
  const promises = Array.from(loadedPlugins.entries())
    .filter(([pluginId]) => pluginStore.isPluginEnabled(pluginId))
    .map(([, plugin]) => plugin.callEvent(event));
  await Promise.allSettled(promises);
}

/**
 * Dispatch a command to a specific browser plugin by ID.
 */
export async function dispatchCommand(
  pluginId: string,
  cmd: string,
  params: unknown,
): Promise<{ success: boolean; data?: unknown; error?: string }> {
  if (!getPluginStore().isPluginEnabled(pluginId)) {
    return { success: false, error: `Plugin is disabled: ${pluginId}` };
  }

  const plugin = loadedPlugins.get(pluginId);
  if (!plugin) {
    return { success: false, error: `Plugin not loaded: ${pluginId}` };
  }
  return plugin.callCommand(cmd, params);
}
