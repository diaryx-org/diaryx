/**
 * Local Workspace Registry — tracks workspaces available on this device.
 *
 * Stored in localStorage so it persists across sessions. All workspaces have
 * stable `local-<uuid>` IDs. Sync status is determined by plugin metadata:
 * a workspace with `pluginMetadata.sync.serverId` is synced.
 *
 * Uses Svelte 5 $state for reactivity — any component using $derived(getLocalWorkspaces())
 * will automatically update when the registry changes.
 */

import { getStorageType, type StorageType } from '$lib/backend/storageType';
import type { WorkspaceEntry } from '$lib/backend/generated';
import { generateUUID } from '$lib/utils';

// ============================================================================
// Types
// ============================================================================

/** Per-plugin metadata stored on a workspace. Keyed by plugin ID. */
export type PluginMetadataMap = Record<string, Record<string, unknown>>;

/**
 * Local workspace — extends the shared core `WorkspaceEntry` with
 * platform-specific fields (storage type, timestamps, plugin metadata).
 *
 * Core fields (`id`, `name`, `path`) come from the generated type.
 */
export interface LocalWorkspace extends WorkspaceEntry {
  /**
   * Whether this workspace is local-only (not synced to server).
   * @deprecated Check `pluginMetadata?.sync?.serverId` instead, or use `isWorkspaceSynced()`.
   * Kept for backward compatibility during migration.
   */
  isLocal: boolean;
  /** Storage backend for this workspace. Undefined = inherit global default. */
  storageType?: StorageType;
  /** When this workspace was first created/downloaded to this device */
  downloadedAt: number;
  /** When the user last opened this workspace */
  lastOpenedAt: number;
  /** Per-plugin metadata. Plugins store opaque data here (e.g. sync stores serverId). */
  pluginMetadata?: PluginMetadataMap;
}

// ============================================================================
// Storage Keys
// ============================================================================

const REGISTRY_KEY = 'diaryx_local_workspaces';
const CURRENT_KEY = 'diaryx_current_workspace';

// ============================================================================
// Reactive State
// ============================================================================

/** In-memory reactive copy of the registry, kept in sync with localStorage. */
let registryState: LocalWorkspace[] = $state(loadFromLocalStorage());
/** Reactive current workspace ID, mirrored to localStorage. */
let currentWorkspaceIdState: string | null = $state(loadCurrentWorkspaceId());

/** Load and migrate the registry from localStorage. */
function loadFromLocalStorage(): LocalWorkspace[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as LocalWorkspace[];

    let needsSave = false;

    for (const ws of parsed) {
      // Migration 1: add isLocal field to entries that don't have it (default true = local)
      if (ws.isLocal === undefined) {
        (ws as any).isLocal = true;
        needsSave = true;
      }

      // Migration 2: server-UUID workspaces → local ID + sync plugin metadata
      // If a workspace has a non-local ID and isLocal === false, it was synced
      // with the old ID scheme. Give it a stable local ID and store the server
      // UUID in pluginMetadata.
      if (!ws.id.startsWith('local-') && !ws.isLocal) {
        const serverUuid = ws.id;
        const newLocalId = `local-${generateUUID()}`;

        // Store server UUID as sync plugin metadata
        if (!ws.pluginMetadata) ws.pluginMetadata = {};
        ws.pluginMetadata['sync'] = {
          ...(ws.pluginMetadata['sync'] || {}),
          serverId: serverUuid,
          syncEnabled: true,
        };

        // Update current workspace ID if it pointed to the old server UUID
        const currentId = localStorage.getItem(CURRENT_KEY);
        if (currentId === serverUuid) {
          localStorage.setItem(CURRENT_KEY, newLocalId);
        }

        ws.id = newLocalId;
        ws.isLocal = true; // isLocal is now always true (derived from plugin metadata)
        needsSave = true;
        console.log(`[WorkspaceRegistry] Migrated workspace "${ws.name}" from server UUID ${serverUuid} to ${newLocalId}`);
      }
    }

    if (needsSave) {
      localStorage.setItem(REGISTRY_KEY, JSON.stringify(parsed));
    }

    return parsed.sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  } catch {
    return [];
  }
}

function loadCurrentWorkspaceId(): string | null {
  if (typeof localStorage === 'undefined') return null;
  return localStorage.getItem(CURRENT_KEY);
}

/** Persist the list to localStorage and update reactive state. */
function saveRegistry(list: LocalWorkspace[]): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(REGISTRY_KEY, JSON.stringify(list));
  // Update $state so Svelte re-renders any $derived consumers
  registryState = list.slice().sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
}

// ============================================================================
// Read Operations
// ============================================================================

/**
 * Get all locally available workspaces, sorted by last opened (most recent first).
 * Returns a reactive $state array — use with $derived() for automatic updates.
 */
export function getLocalWorkspaces(): LocalWorkspace[] {
  return registryState;
}

/**
 * Get a single local workspace by ID.
 */
export function getLocalWorkspace(id: string): LocalWorkspace | null {
  return registryState.find(w => w.id === id) ?? null;
}

/**
 * Get the currently active workspace ID, or null if none selected.
 */
export function getCurrentWorkspaceId(): string | null {
  return currentWorkspaceIdState;
}

/**
 * Check whether a workspace is downloaded locally (exists in the registry).
 */
export function isWorkspaceLocal(id: string): boolean {
  return registryState.some(w => w.id === id);
}

/**
 * Check whether a workspace is synced to a server.
 * A workspace is synced if it has sync plugin metadata with a serverId.
 */
export function isWorkspaceSynced(id: string): boolean {
  const ws = registryState.find(w => w.id === id);
  return !!ws?.pluginMetadata?.['sync']?.serverId;
}

/**
 * Get the server workspace ID for a synced workspace, or null if not synced.
 */
export function getServerWorkspaceId(id: string): string | null {
  const ws = registryState.find(w => w.id === id);
  const serverId = ws?.pluginMetadata?.['sync']?.serverId;
  return typeof serverId === 'string' ? serverId : null;
}

/**
 * Get the storage type for a workspace, falling back to the global default.
 */
export function getWorkspaceStorageType(id: string): StorageType {
  const ws = registryState.find(w => w.id === id);
  return ws?.storageType ?? getStorageType();
}

/**
 * Set the storage type for a specific workspace.
 */
export function setWorkspaceStorageType(id: string, type: StorageType): void {
  const list = [...registryState];
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.storageType = type;
    saveRegistry(list);
  }
}

// ============================================================================
// Write Operations
// ============================================================================

/**
 * Register a workspace as locally available.
 * If it already exists, updates the name.
 */
export function addLocalWorkspace(ws: { id: string; name: string; isLocal?: boolean; storageType?: StorageType; path?: string }): void {
  const list = [...registryState];
  const existing = list.find(w => w.id === ws.id);
  if (existing) {
    existing.name = ws.name;
    if (ws.isLocal !== undefined) {
      existing.isLocal = ws.isLocal;
    }
    if (ws.storageType !== undefined) {
      existing.storageType = ws.storageType;
    }
    if (ws.path !== undefined) {
      existing.path = ws.path;
    }
  } else {
    list.push({
      id: ws.id,
      name: ws.name,
      isLocal: ws.isLocal ?? true,
      storageType: ws.storageType,
      ...(ws.path ? { path: ws.path } : {}),
      downloadedAt: Date.now(),
      lastOpenedAt: Date.now(),
    });
  }
  saveRegistry(list);
}

/**
 * Remove a workspace from the local registry (e.g. "remove local copy").
 * Does NOT delete server data.
 */
export function removeLocalWorkspace(id: string): void {
  const list = registryState.filter(w => w.id !== id);
  saveRegistry(list);

  // If we removed the current workspace, clear the selection
  if (getCurrentWorkspaceId() === id) {
    clearCurrentWorkspaceId();
  }
}

/**
 * Set the currently active workspace.
 * Also updates `lastOpenedAt` and keeps localStorage workspace name in sync.
 */
export function setCurrentWorkspaceId(id: string): void {
  currentWorkspaceIdState = id;
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(CURRENT_KEY, id);
  }

  // Update lastOpenedAt and keep workspace name in sync
  const list = [...registryState];
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.lastOpenedAt = Date.now();
    saveRegistry(list);
    // Keep localStorage workspace name in sync for page reloads
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('diaryx-workspace-name', ws.name);
    }
  }
}

/**
 * Clear the current workspace selection.
 */
export function clearCurrentWorkspaceId(): void {
  currentWorkspaceIdState = null;
  if (typeof localStorage === 'undefined') return;
  localStorage.removeItem(CURRENT_KEY);
}

/**
 * Rename a workspace in the local registry.
 */
export function renameLocalWorkspace(id: string, newName: string): void {
  const list = [...registryState];
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.name = newName;
    saveRegistry(list);
    // If this is the current workspace, update localStorage name too
    if (getCurrentWorkspaceId() === id) {
      localStorage.setItem('diaryx-workspace-name', newName);
    }
  }
}

/**
 * Set sync status for a workspace via plugin metadata.
 * When synced: stores serverId in pluginMetadata.sync.
 * When unsynced: removes the sync plugin metadata.
 * @deprecated Use setPluginMetadata() with "sync" plugin ID directly.
 */
export function setWorkspaceIsLocal(id: string, isLocal: boolean): void {
  if (isLocal) {
    // Remove sync metadata to mark as local-only
    setPluginMetadata(id, 'sync', null);
  }
  // If setting to synced (isLocal=false), the caller should also call
  // setPluginMetadata(id, 'sync', { serverId: ... }) to provide the server ID.
  // The isLocal field is kept in sync for backward compat.
  const list = [...registryState];
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.isLocal = isLocal;
    saveRegistry(list);
  }
}

// ============================================================================
// Plugin Storage Helpers
// ============================================================================

/**
 * Get the storage plugin ID for a workspace, if it uses plugin storage.
 * Returns undefined if the workspace doesn't use plugin storage.
 */
export function getWorkspaceStoragePluginId(id: string): string | undefined {
  const ws = registryState.find(w => w.id === id);
  if (ws?.storageType !== 'plugin') return undefined;
  return ws?.pluginMetadata?.['storage']?.pluginId as string | undefined;
}

/**
 * Set a workspace to use plugin storage with the given plugin ID.
 * Updates both the storageType and plugin metadata.
 */
export function setWorkspaceStoragePlugin(id: string, pluginId: string): void {
  const list = [...registryState];
  const ws = list.find(w => w.id === id);
  if (!ws) return;

  ws.storageType = 'plugin';
  if (!ws.pluginMetadata) ws.pluginMetadata = {};
  ws.pluginMetadata['storage'] = { pluginId };
  saveRegistry(list);
}

// ============================================================================
// Plugin Metadata
// ============================================================================

/**
 * Get plugin metadata for a workspace.
 * Returns the metadata object for the given plugin, or undefined if not set.
 */
export function getPluginMetadata(workspaceId: string, pluginId: string): Record<string, unknown> | undefined {
  const ws = registryState.find(w => w.id === workspaceId);
  return ws?.pluginMetadata?.[pluginId];
}

/**
 * Set plugin metadata for a workspace.
 * Merges the provided data into the existing metadata for the plugin.
 * Pass null to remove the plugin's metadata entirely.
 */
export function setPluginMetadata(workspaceId: string, pluginId: string, data: Record<string, unknown> | null): void {
  const list = [...registryState];
  const ws = list.find(w => w.id === workspaceId);
  if (!ws) return;

  if (data === null) {
    // Remove plugin metadata
    if (ws.pluginMetadata) {
      delete ws.pluginMetadata[pluginId];
      if (Object.keys(ws.pluginMetadata).length === 0) {
        delete ws.pluginMetadata;
      }
    }
  } else {
    // Merge plugin metadata
    if (!ws.pluginMetadata) {
      ws.pluginMetadata = {};
    }
    ws.pluginMetadata[pluginId] = {
      ...(ws.pluginMetadata[pluginId] || {}),
      ...data,
    };
  }

  // Keep isLocal in sync for backward compat
  if (pluginId === 'sync') {
    ws.isLocal = !ws.pluginMetadata?.['sync']?.serverId;
  }

  saveRegistry(list);
}

// ============================================================================
// Local Workspace Operations
// ============================================================================

/**
 * Create a new local-only workspace (no server required).
 * Returns the created workspace entry.
 */
export function createLocalWorkspace(name: string, storageType?: StorageType, path?: string): LocalWorkspace {
  const id = `local-${generateUUID()}`;
  const ws: LocalWorkspace = {
    id,
    name,
    isLocal: true,
    storageType: storageType ?? getStorageType(),
    ...(path ? { path } : {}),
    downloadedAt: Date.now(),
    lastOpenedAt: Date.now(),
  };
  const list = [...registryState, ws];
  saveRegistry(list);
  return ws;
}

/**
 * Link a local workspace to a server workspace by storing the server UUID
 * in the sync plugin metadata. The local workspace ID stays stable.
 *
 * @deprecated Use setPluginMetadata(workspaceId, 'sync', { serverId, syncEnabled: true }) directly.
 */
export function promoteLocalWorkspace(localId: string, serverUuid: string): void {
  setPluginMetadata(localId, 'sync', {
    serverId: serverUuid,
    syncEnabled: true,
  });
}

/**
 * Bootstrap the registry for first-time users.
 * If the registry is empty and a "My Journal" OPFS dir exists (or we're in a fresh state),
 * creates a default local-only workspace entry.
 */
export function bootstrapDefaultWorkspace(): LocalWorkspace {
  const list = registryState;
  if (list.length > 0) {
    // Registry already has entries — return the current or first workspace
    const currentId = getCurrentWorkspaceId();
    const current = currentId ? list.find(w => w.id === currentId) : null;
    return current ?? list[0];
  }

  // Create a default local-only workspace
  const name = localStorage.getItem('diaryx-workspace-name') || 'My Journal';
  const ws = createLocalWorkspace(name);
  setCurrentWorkspaceId(ws.id);
  return ws;
}

// ============================================================================
// Auto-Discovery
// ============================================================================

/** OPFS directories that are known non-workspace system dirs. */
const SYSTEM_DIRS = new Set(['diaryx', '.diaryx', 'guest']);

/**
 * Scan the OPFS root for workspace directories that aren't in the registry yet.
 * Each top-level OPFS directory (excluding system dirs) is assumed to be a workspace.
 * Discovered workspaces are added to the registry as local-only.
 *
 * Returns the list of newly discovered workspaces.
 */
export async function discoverOpfsWorkspaces(): Promise<LocalWorkspace[]> {
  if (typeof navigator === 'undefined' || !navigator.storage?.getDirectory) return [];

  try {
    const root = await navigator.storage.getDirectory();
    const knownNames = new Set(registryState.map(w => w.name));
    const discovered: LocalWorkspace[] = [];

    for await (const [name, handle] of (root as any).entries()) {
      if (handle.kind !== 'directory') continue;
      if (SYSTEM_DIRS.has(name)) continue;
      if (knownNames.has(name)) continue;

      // This is an unregistered workspace directory — add it (OPFS since discovered there)
      const ws = createLocalWorkspace(name, 'opfs');
      discovered.push(ws);
      knownNames.add(name);
      console.log(`[WorkspaceRegistry] Discovered unregistered workspace: ${name}`);
    }

    return discovered;
  } catch (e) {
    console.warn('[WorkspaceRegistry] Failed to discover OPFS workspaces:', e);
    return [];
  }
}
