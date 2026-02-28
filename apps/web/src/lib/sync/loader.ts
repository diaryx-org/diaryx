/**
 * On-demand loader for the Extism sync plugin.
 *
 * Fetches and instantiates the `diaryx_sync.wasm` plugin lazily when sync
 * is first enabled. The WASM file is served from `/plugins/diaryx_sync.wasm`
 * (bundled in `public/plugins/` for self-hosted deployments) and cached by
 * the browser after first load.
 *
 * ## Usage
 *
 * ```ts
 * import { loadSyncPlugin } from '$lib/sync/loader';
 * import type { Backend } from '$lib/backend/interface';
 *
 * const plugin = await loadSyncPlugin(backend);
 * await plugin.init({ workspaceId: 'my-ws', writeToDisk: true });
 * ```
 */

import { createPlugin } from '@extism/extism';
import { ExtismSyncPlugin } from './extismSyncPlugin';
import { buildHostFunctions, type SyncHostContext } from './hostFunctions';
import type { Backend } from '../backend/interface';
import type {
  PluginManifest,
  PluginCapability,
  UiContribution,
} from '$lib/backend/generated';
import { getPluginStore } from '@/models/stores/pluginStore.svelte';
import {
  registerSyncWsHandler,
  unregisterSyncWsHandler,
} from './syncWsRegistry';
import { createExtismSyncWsHandlerFactory } from './extismSyncBackend';
import { SYNC_PLUGIN_ID } from './syncBuiltinUiRegistry';

// ============================================================================
// Configuration
// ============================================================================

/** Default path to the sync plugin WASM file. */
const DEFAULT_WASM_URL = '/plugins/diaryx_sync.wasm';
const WASM_BINDGEN_MARKERS = [
  '__wbindgen_placeholder__',
  '__wbindgen_object_drop_ref',
];

/** Singleton promise to prevent concurrent loads. */
let _loadPromise: Promise<ExtismSyncPlugin> | null = null;

/** Singleton instance after successful load. */
let _instance: ExtismSyncPlugin | null = null;

/** Runtime manifest plugin id currently registered in pluginStore. */
let _runtimeManifestPluginId: string | null = null;

/** Sync WS handler plugin id currently registered in the registry. */
let _registeredSyncHandlerPluginId: string | null = null;

// ============================================================================
// Public API
// ============================================================================

export interface LoadSyncPluginOptions {
  /** URL to fetch the WASM file from. Defaults to `/plugins/diaryx_sync.wasm`. */
  wasmUrl?: string;
  /** Callback for sync events emitted by the guest plugin. */
  onSyncEvent?: (eventJson: string) => void;
}

/**
 * Load the Extism sync plugin on demand.
 *
 * Returns a cached instance if already loaded. Concurrent calls will
 * share the same loading promise.
 *
 * @param backend - Backend for filesystem access (passed to host functions)
 * @param options - Optional configuration
 * @returns Initialized ExtismSyncPlugin instance
 */
export async function loadSyncPlugin(
  backend: Backend,
  options: LoadSyncPluginOptions = {},
): Promise<ExtismSyncPlugin> {
  if (_instance) return _instance;
  if (_loadPromise) return _loadPromise;

  _loadPromise = doLoad(backend, options);

  try {
    _instance = await _loadPromise;
    return _instance;
  } catch (e) {
    _loadPromise = null;
    throw e;
  }
}

/**
 * Get the currently loaded sync plugin, or null if not yet loaded.
 */
export function getSyncPlugin(): ExtismSyncPlugin | null {
  return _instance;
}

/**
 * Check if the sync plugin is loaded.
 */
export function isSyncPluginLoaded(): boolean {
  return _instance !== null;
}

/**
 * Unload the sync plugin and release resources.
 */
export async function unloadSyncPlugin(): Promise<void> {
  if (_runtimeManifestPluginId) {
    getPluginStore().clearRuntimeManifestOverride(_runtimeManifestPluginId);
    _runtimeManifestPluginId = null;
  }
  if (_registeredSyncHandlerPluginId) {
    unregisterSyncWsHandler(_registeredSyncHandlerPluginId);
    _registeredSyncHandlerPluginId = null;
  }

  if (_instance) {
    try {
      await _instance.shutdown();
    } catch (e) {
      console.warn('[SyncPluginLoader] Error during shutdown:', e);
    }
    try {
      await _instance.close();
    } catch (e) {
      console.warn('[SyncPluginLoader] Error closing plugin:', e);
    }
    _instance = null;
  }
  _loadPromise = null;
}

// ============================================================================
// Internal
// ============================================================================

async function doLoad(
  backend: Backend,
  options: LoadSyncPluginOptions,
): Promise<ExtismSyncPlugin> {
  const wasmUrl = options.wasmUrl ?? DEFAULT_WASM_URL;

  console.log(`[SyncPluginLoader] Loading sync plugin from: ${wasmUrl}`);

  const hostCtx: SyncHostContext = {
    backend,
    onSyncEvent: options.onSyncEvent,
  };

  const hostFunctions = buildHostFunctions(hostCtx);

  // Fetch the WASM file
  const response = await fetch(wasmUrl);
  if (!response.ok) {
    throw new Error(
      `[SyncPluginLoader] Failed to fetch sync plugin: ${response.status} ${response.statusText}`,
    );
  }
  const wasmBytes = new Uint8Array(await response.arrayBuffer());
  assertExtismCompatibleWasm(wasmBytes, wasmUrl);

  console.log(
    `[SyncPluginLoader] WASM loaded: ${(wasmBytes.byteLength / 1024).toFixed(0)} KB`,
  );

  // Create the Extism plugin with host functions
  let extismPlugin;
  try {
    extismPlugin = await createPlugin(
      { wasm: [{ data: wasmBytes }] },
      {
        useWasi: false,
        functions: hostFunctions,
        logLevel: 'info',
      },
    );
  } catch (error) {
    const err = error instanceof Error ? error : new Error(String(error));
    throw new Error(
      `[SyncPluginLoader] Failed to instantiate Extism plugin from ${wasmUrl}: ${err.message}`,
      { cause: error },
    );
  }

  const plugin = new ExtismSyncPlugin(extismPlugin);

  // Verify the manifest
  const guestManifest = await plugin.getManifest();
  const manifest = convertGuestManifest(guestManifest);

  const pluginId = String(manifest.id);
  if (pluginId === SYNC_PLUGIN_ID) {
    // Runtime sync manifest should override backend-provided UI contributions.
    getPluginStore().setRuntimeManifestOverride(manifest);
    _runtimeManifestPluginId = pluginId;
  }

  registerSyncWsHandler(pluginId, createExtismSyncWsHandlerFactory(plugin));
  _registeredSyncHandlerPluginId = pluginId;

  console.log(
    `[SyncPluginLoader] Plugin loaded: ${manifest.name} v${manifest.version} (${manifest.id})`,
  );

  return plugin;
}

function assertExtismCompatibleWasm(bytes: Uint8Array, wasmUrl: string): void {
  for (const marker of WASM_BINDGEN_MARKERS) {
    if (containsAscii(bytes, marker)) {
      throw new Error(
        `[SyncPluginLoader] ${wasmUrl} is not Extism-compatible (found ${marker}). ` +
          'Rebuild/copy crates/diaryx_sync_extism output into apps/web/public/plugins/diaryx_sync.wasm.',
      );
    }
  }
}

function containsAscii(haystack: Uint8Array, needle: string): boolean {
  const bytes = new TextEncoder().encode(needle);
  if (bytes.length === 0 || bytes.length > haystack.length) return false;

  outer: for (let i = 0; i <= haystack.length - bytes.length; i++) {
    for (let j = 0; j < bytes.length; j++) {
      if (haystack[i + j] !== bytes[j]) continue outer;
    }
    return true;
  }

  return false;
}

function convertGuestManifest(guest: {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities?: string[];
  ui?: unknown[];
  commands?: string[];
}): PluginManifest {
  const commands = guest.commands ?? [];
  const capabilities: PluginCapability[] = (guest.capabilities ?? [])
    .map((cap): PluginCapability | null => {
      switch (cap) {
        case 'file_events':
          return 'FileEvents';
        case 'workspace_events':
          return 'WorkspaceEvents';
        case 'crdt_commands':
          return 'CrdtCommands';
        case 'sync_transport':
          return 'SyncTransport';
        case 'custom_commands':
          return { CustomCommands: { commands } };
        default:
          console.warn(`[SyncPluginLoader] Unknown capability from guest manifest: ${cap}`);
          return null;
      }
    })
    .filter((cap): cap is PluginCapability => cap !== null);

  return {
    id: guest.id,
    name: guest.name,
    version: guest.version,
    description: guest.description,
    capabilities,
    ui: (guest.ui ?? []) as UiContribution[],
  };
}
