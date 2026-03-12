/**
 * Browser Plugin Manager — manages the lifecycle of browser-loaded Extism plugins.
 *
 * Plugins are stored in the current workspace under `.diaryx/plugins/` and
 * loaded at startup. Legacy IndexedDB installs are migrated into the workspace
 * on demand. Their manifests are merged into the pluginStore alongside any
 * manifests from the native backend.
 */

import {
  loadBrowserPlugin,
  inspectBrowserPlugin,
  getBrowserPluginRuntimeSupport,
  type BrowserPluginCallOptions,
  type BrowserExtismPlugin,
  type GuestEvent,
  type RequestedPermissionsManifest,
} from "./extismBrowserLoader";
import type { PluginManifest } from "$lib/backend/generated";
import { getPluginStore } from "@/models/stores/pluginStore.svelte";
import type {
  PluginConfig,
  PluginPermissions,
} from "@/models/stores/permissionStore.svelte";
import {
  createExtensionFromManifest,
  createMarkFromManifest,
  getBuiltinExtension,
  isEditorExtension,
  type EditorExtensionManifest,
  type RenderFn,
} from "./editorExtensionFactory";
import {
  clearPreservedPluginEditorExtensions,
  preservePluginEditorExtensions,
} from "./preservedEditorExtensions.svelte";
import {
  deleteWorkspaceTree,
  getPluginInstallPath,
  listWorkspaceFiles,
  readWorkspaceBinary,
  writeWorkspaceBinary,
} from "$lib/workspace/workspaceAssetStorage";

// ============================================================================
// Legacy IndexedDB storage (migration only)
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

async function migrateLegacyIndexedDbPlugins(): Promise<void> {
  const stored = await getAllStoredPlugins();
  await Promise.all(
    stored.map(async (entry) => {
      const pluginPath = getPluginInstallPath(entry.id);
      const existing = await readWorkspaceBinary(pluginPath);
      if (!existing) {
        await writeWorkspaceBinary(pluginPath, new Uint8Array(entry.wasm));
      }
    }),
  );
}

async function listWorkspacePluginWasmPaths(): Promise<string[]> {
  const files = await listWorkspaceFiles(".diaryx/plugins");
  return files
    .filter((path) => path.endsWith("/plugin.wasm"))
    .sort((a, b) => a.localeCompare(b));
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

/** Current workspace plugin permission config provider. */
let pluginsConfigProvider:
  | (() => Record<string, PluginConfig> | undefined)
  | null = null;

/** Persists plugin-declared default permissions into root frontmatter. */
let pluginsConfigPersistor:
  | ((pluginId: string, defaults: PluginPermissions) => Promise<void>)
  | null = null;

/** Reactive version counter — incremented when browser plugins finish loading. */
let pluginExtensionsVersion = $state(0);

type PluginEventObserver = (event: GuestEvent) => Promise<void> | void;
const pluginEventObservers = new Set<PluginEventObserver>();

function normalizePluginEventPath(path: string): string {
  return path
    .replace(/\\/g, "/")
    .replace(/\/+/g, "/")
    .replace(/^\.\//, "");
}

function invalidateEditorExtensions(): void {
  cachedEditorExtensions = null;
  cachedPluginCount = 0;
  pluginExtensionsVersion++;
}

async function unloadAllPlugins(): Promise<void> {
  await Promise.allSettled(
    Array.from(loadedPlugins.values()).map((plugin) => plugin.close()),
  );
  loadedPlugins.clear();
  browserManifests = [];
  invalidateEditorExtensions();
}

function hasRequestedPermissionDefaults(
  defaults: PluginPermissions | undefined,
): defaults is PluginPermissions {
  if (!defaults) return false;
  return Object.values(defaults).some((rule) => rule != null);
}

async function persistRequestedPermissionDefaults(
  pluginId: string,
  requestedPermissions?: RequestedPermissionsManifest,
): Promise<void> {
  const defaults = requestedPermissions?.defaults;
  if (!pluginsConfigPersistor || !hasRequestedPermissionDefaults(defaults)) {
    return;
  }

  await pluginsConfigPersistor(pluginId, defaults);
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Install a plugin from raw WASM bytes.
 *
 * The bytes are persisted in the current workspace and the plugin is loaded immediately.
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

  const runtimeIdentity = {
    pluginId: "unknown-plugin",
    pluginName: name ?? "Unknown Plugin",
  };
  console.log(
    `[browserPluginManager] Installing plugin (${(wasmBytes.byteLength / 1024).toFixed(0)} KB)...`,
  );
  const inspected = await inspectBrowserPlugin(wasmBytes);
  await persistRequestedPermissionDefaults(
    String(inspected.manifest.id),
    inspected.requestedPermissions,
  );
  const plugin = await loadBrowserPlugin(wasmBytes, {
    getPluginId: () => runtimeIdentity.pluginId,
    getPluginName: () => runtimeIdentity.pluginName,
    getPluginsConfig: () => pluginsConfigProvider?.(),
  });
  const id = plugin.manifest.id as unknown as string;
  runtimeIdentity.pluginId = id;
  runtimeIdentity.pluginName = String(plugin.manifest.name ?? id);
  clearPreservedPluginEditorExtensions(id);
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

  await writeWorkspaceBinary(getPluginInstallPath(id), new Uint8Array(wasmBytes));

  // Track loaded instance.
  loadedPlugins.set(id, plugin);
  browserManifests = [...browserManifests, plugin.manifest];
  invalidateEditorExtensions();

  return plugin.manifest;
}

/**
 * Uninstall a plugin by ID.
 *
 * Closes the plugin instance and removes it from the current workspace.
 */
export async function uninstallPlugin(pluginId: string): Promise<void> {
  const removedManifest =
    loadedPlugins.get(pluginId)?.manifest ??
    browserManifests.find((m) => (m.id as unknown as string) === pluginId);
  const plugin = loadedPlugins.get(pluginId);
  if (plugin) {
    await plugin.close();
    loadedPlugins.delete(pluginId);
  }
  await deleteWorkspaceTree(`.diaryx/plugins/${pluginId}`);
  browserManifests = browserManifests.filter(
    (m) => (m.id as unknown as string) !== pluginId,
  );
  getPluginStore().clearPluginEnabled(pluginId);
  preservePluginEditorExtensions(removedManifest);
  invalidateEditorExtensions();
}

/**
 * Load all installed plugins from the current workspace.
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
    await unloadAllPlugins();
    await migrateLegacyIndexedDbPlugins();

    const stored = await listWorkspacePluginWasmPaths();
    console.log(
      `[browserPluginManager] Found ${stored.length} workspace plugin(s)`,
    );
    for (const pluginPath of stored) {
      try {
        const wasmBytes = await readWorkspaceBinary(pluginPath);
        if (!wasmBytes) {
          continue;
        }
        const runtimeIdentity = {
          pluginId: pluginPath,
          pluginName: pluginPath,
        };
        const wasmBuffer = wasmBytes.buffer.slice(
          wasmBytes.byteOffset,
          wasmBytes.byteOffset + wasmBytes.byteLength,
        ) as ArrayBuffer;
        const inspected = await inspectBrowserPlugin(wasmBuffer);
        await persistRequestedPermissionDefaults(
          String(inspected.manifest.id),
          inspected.requestedPermissions,
        );
        const plugin = await loadBrowserPlugin(wasmBuffer, {
          getPluginId: () => runtimeIdentity.pluginId,
          getPluginName: () => runtimeIdentity.pluginName,
          getPluginsConfig: () => pluginsConfigProvider?.(),
        });
        const id = plugin.manifest.id as unknown as string;
        runtimeIdentity.pluginId = id;
        runtimeIdentity.pluginName = String(plugin.manifest.name ?? id);
        clearPreservedPluginEditorExtensions(id);
        loadedPlugins.set(id, plugin);
        browserManifests = [...browserManifests, plugin.manifest];
        console.log(
          `[browserPluginManager] Loaded plugin: ${id} (${plugin.manifest.name}) v${plugin.manifest.version}`,
        );
      } catch (e) {
        console.warn(
          `[browserPluginManager] Failed to load plugin from ${pluginPath}:`,
          e,
        );
      }
    }
  } catch (e) {
    console.warn(
      "[browserPluginManager] Failed to load workspace plugins:",
      e,
    );
  }

  invalidateEditorExtensions();
}

/** Configure how browser plugins read workspace permission config. */
export function setPluginPermissionConfigProvider(
  provider: (() => Record<string, PluginConfig> | undefined) | null,
): void {
  pluginsConfigProvider = provider;
}

/** Configure how browser plugins persist requested default permissions. */
export function setPluginPermissionConfigPersistor(
  persistor:
    | ((pluginId: string, defaults: PluginPermissions) => Promise<void>)
    | null,
): void {
  pluginsConfigPersistor = persistor;
}

/** Inspect plugin manifest metadata without installing. */
export async function inspectPluginWasm(
  wasmBytes: ArrayBuffer,
): Promise<{
  pluginId: string;
  pluginName: string;
  requestedPermissions?: RequestedPermissionsManifest;
}> {
  const inspected = await inspectBrowserPlugin(wasmBytes);
  return {
    pluginId: inspected.manifest.id as unknown as string,
    pluginName: String(inspected.manifest.name ?? inspected.manifest.id),
    requestedPermissions: inspected.requestedPermissions,
  };
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

/**
 * Reactive version counter for plugin extensions.
 * Incremented each time browser plugins finish loading.
 * Used by the editor to detect when extensions need rebuilding.
 */
export function getPluginExtensionsVersion(): number {
  return pluginExtensionsVersion;
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
          const manifest = ui as EditorExtensionManifest;

          // Check for Builtin node type — use host-registered extensions
          if (
            typeof manifest.node_type === "object" &&
            "Builtin" in manifest.node_type
          ) {
            const builtinExts = getBuiltinExtension(
              manifest.node_type.Builtin.host_extension_id,
            );
            if (builtinExts) {
              extensions.push(...builtinExts);
            }
            continue;
          }

          if (manifest.node_type === "InlineMark") {
            extensions.push(createMarkFromManifest(manifest));
          } else if (manifest.render_export) {
            const renderExport = manifest.render_export;
            const renderFn: RenderFn = (source, displayMode) =>
              plugin.callRender(renderExport, source, {
                display_mode: displayMode,
              });
            extensions.push(
              createExtensionFromManifest(manifest, renderFn),
            );
          }
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
  if (pluginEventObservers.size > 0) {
    await Promise.allSettled(
      Array.from(pluginEventObservers).map((observer) => observer(event)),
    );
  }
}

export function onPluginEventDispatched(
  observer: PluginEventObserver,
): () => void {
  pluginEventObservers.add(observer);
  return () => {
    pluginEventObservers.delete(observer);
  };
}

export async function dispatchFileOpenedEvent(path: string): Promise<void> {
  await dispatchEvent({
    event_type: "file_opened",
    payload: { path: normalizePluginEventPath(path) },
  });
}

export async function dispatchFileSavedEvent(
  path: string,
  options?: { bodyChanged?: boolean },
): Promise<void> {
  await dispatchEvent({
    event_type: "file_saved",
    payload: {
      path: normalizePluginEventPath(path),
      body_changed: options?.bodyChanged ?? true,
    },
  });
}

export async function dispatchFileCreatedEvent(path: string): Promise<void> {
  await dispatchEvent({
    event_type: "file_created",
    payload: { path: normalizePluginEventPath(path) },
  });
}

export async function dispatchFileDeletedEvent(path: string): Promise<void> {
  await dispatchEvent({
    event_type: "file_deleted",
    payload: { path: normalizePluginEventPath(path) },
  });
}

export async function dispatchFileMovedEvent(
  oldPath: string,
  newPath: string,
): Promise<void> {
  await dispatchEvent({
    event_type: "file_moved",
    payload: {
      old_path: normalizePluginEventPath(oldPath),
      new_path: normalizePluginEventPath(newPath),
    },
  });
}

/**
 * Dispatch a command to a specific browser plugin by ID.
 */
export async function dispatchCommand(
  pluginId: string,
  cmd: string,
  params: unknown,
  options?: BrowserPluginCallOptions,
): Promise<{ success: boolean; data?: unknown; error?: string }> {
  const startedAt = performance.now();
  if (!getPluginStore().isPluginEnabled(pluginId)) {
    console.warn("[browserPluginManager] dispatch blocked (disabled plugin)", {
      pluginId,
      cmd,
    });
    return { success: false, error: `Plugin is disabled: ${pluginId}` };
  }

  const plugin = loadedPlugins.get(pluginId);
  if (!plugin) {
    console.warn("[browserPluginManager] dispatch blocked (plugin not loaded)", {
      pluginId,
      cmd,
      loadedPluginCount: loadedPlugins.size,
    });
    return { success: false, error: `Plugin not loaded: ${pluginId}` };
  }
  console.debug("[browserPluginManager] dispatch start", {
    pluginId,
    cmd,
  });
  const result = await plugin.callCommand(cmd, params, options);
  console.debug("[browserPluginManager] dispatch done", {
    pluginId,
    cmd,
    elapsedMs: Math.round(performance.now() - startedAt),
    success: result.success,
    hasData: result.data != null,
    error: result.error ?? null,
  });
  return result;
}
