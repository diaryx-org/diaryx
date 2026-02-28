/**
 * Browser Plugin Manager — manages the lifecycle of browser-loaded Extism plugins.
 *
 * Plugins are stored as .wasm blobs in IndexedDB and loaded at startup.
 * Their manifests are merged into the pluginStore alongside any manifests
 * from the native backend.
 */

import {
  loadBrowserPlugin,
  type BrowserExtismPlugin,
  type GuestEvent,
} from './extismBrowserLoader';
import type { PluginManifest } from '$lib/backend/generated';

// ============================================================================
// IndexedDB storage
// ============================================================================

const DB_NAME = 'diaryx-plugins';
const DB_VERSION = 1;
const STORE_NAME = 'plugins';

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
        db.createObjectStore(STORE_NAME, { keyPath: 'id' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function getAllStoredPlugins(): Promise<StoredPlugin[]> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const store = tx.objectStore(STORE_NAME);
    const request = store.getAll();
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function storePlugin(entry: StoredPlugin): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    const store = tx.objectStore(STORE_NAME);
    const request = store.put(entry);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
}

async function removeStoredPlugin(id: string): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
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
  console.log(`[browserPluginManager] Installing plugin (${(wasmBytes.byteLength / 1024).toFixed(0)} KB)...`);
  const plugin = await loadBrowserPlugin(wasmBytes);
  const id = plugin.manifest.id as unknown as string;
  console.log(`[browserPluginManager] Installed plugin: ${id} (${plugin.manifest.name})`);

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
}

/**
 * Load all installed plugins from IndexedDB.
 *
 * Called once at app startup. Plugins that fail to load are logged and skipped.
 */
export async function loadAllPlugins(): Promise<void> {
  try {
    const stored = await getAllStoredPlugins();
    console.log(`[browserPluginManager] Found ${stored.length} stored plugin(s)`);
    for (const entry of stored) {
      try {
        const plugin = await loadBrowserPlugin(entry.wasm);
        const id = plugin.manifest.id as unknown as string;
        loadedPlugins.set(id, plugin);
        browserManifests = [...browserManifests, plugin.manifest];
        console.log(`[browserPluginManager] Loaded plugin: ${id} (${plugin.manifest.name})`);
      } catch (e) {
        console.warn(
          `[browserPluginManager] Failed to load plugin ${entry.id}:`,
          e,
        );
      }
    }
  } catch (e) {
    console.warn('[browserPluginManager] Failed to load plugins from IndexedDB:', e);
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

// ============================================================================
// Event dispatch helpers
// ============================================================================

/**
 * Dispatch a lifecycle event to all loaded browser plugins.
 */
export async function dispatchEvent(event: GuestEvent): Promise<void> {
  const promises = Array.from(loadedPlugins.values()).map((plugin) =>
    plugin.callEvent(event),
  );
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
  const plugin = loadedPlugins.get(pluginId);
  if (!plugin) {
    return { success: false, error: `Plugin not loaded: ${pluginId}` };
  }
  return plugin.callCommand(cmd, params);
}
