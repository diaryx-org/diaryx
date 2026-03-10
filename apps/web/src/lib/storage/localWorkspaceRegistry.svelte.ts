/**
 * Local Workspace Registry — tracks workspaces available on this device.
 *
 * Stored in localStorage so it persists across sessions. All workspaces have
 * stable `local-<uuid>` IDs. Remote workspace links are stored in per-plugin
 * metadata, so hosts only need to understand opaque provider IDs + remote IDs.
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

/** Normalized remote-workspace link stored in host registry state. */
export interface WorkspaceProviderLink {
  pluginId: string;
  remoteWorkspaceId: string;
  syncEnabled: boolean;
}

/**
 * Local workspace — extends the shared core `WorkspaceEntry` with
 * platform-specific fields (storage type, timestamps, plugin metadata).
 *
 * Core fields (`id`, `name`, `path`) come from the generated type.
 */
export interface LocalWorkspace extends WorkspaceEntry {
  /**
   * Whether this workspace is local-only (not synced to server).
   * @deprecated Check provider link metadata (`remoteWorkspaceId` / legacy `serverId`)
   * instead, or use
   * `isWorkspaceSynced()`.
   * Kept for backward compatibility during migration.
   */
  isLocal: boolean;
  /** Storage backend for this workspace. Undefined = inherit global default. */
  storageType?: StorageType;
  /** When this workspace was first created/downloaded to this device */
  downloadedAt: number;
  /** When the user last opened this workspace */
  lastOpenedAt: number;
  /** Per-plugin metadata. Providers can store remote workspace link state here. */
  pluginMetadata?: PluginMetadataMap;
}

// ============================================================================
// Storage Keys
// ============================================================================

const REGISTRY_KEY = 'diaryx_local_workspaces';
const CURRENT_KEY = 'diaryx_current_workspace';
const SYNC_PLUGIN_ID = 'diaryx.sync';
const LEGACY_SYNC_PLUGIN_ID = 'sync';

// ============================================================================
// Reactive State
// ============================================================================

/** In-memory reactive copy of the registry, kept in sync with localStorage. */
let registryState: LocalWorkspace[] = $state(loadFromLocalStorage());
/** Reactive current workspace ID, mirrored to localStorage. */
let currentWorkspaceIdState: string | null = $state(loadCurrentWorkspaceId());

function readSyncMetadata(
  ws: LocalWorkspace | undefined | null,
): Record<string, unknown> | undefined {
  return ws?.pluginMetadata?.[SYNC_PLUGIN_ID]
    ?? ws?.pluginMetadata?.[LEGACY_SYNC_PLUGIN_ID];
}

function normalizeProviderPluginId(pluginId: string): string {
  return pluginId === LEGACY_SYNC_PLUGIN_ID ? SYNC_PLUGIN_ID : pluginId;
}

function readRemoteWorkspaceId(
  metadata: Record<string, unknown> | undefined,
): string | null {
  const remoteWorkspaceId = metadata?.remoteWorkspaceId;
  if (typeof remoteWorkspaceId === 'string' && remoteWorkspaceId.trim().length > 0) {
    return remoteWorkspaceId;
  }

  const serverId = metadata?.serverId;
  if (typeof serverId === 'string' && serverId.trim().length > 0) {
    return serverId;
  }

  return null;
}

function getWorkspaceProviderLinksFromWorkspace(
  ws: LocalWorkspace | undefined | null,
): WorkspaceProviderLink[] {
  const pluginMetadata = ws?.pluginMetadata;
  if (!pluginMetadata) return [];

  const links: WorkspaceProviderLink[] = [];
  for (const [pluginId, metadata] of Object.entries(pluginMetadata)) {
    if (!metadata || typeof metadata !== 'object') continue;

    const effectivePluginId = normalizeProviderPluginId(pluginId);
    if (links.some((entry) => entry.pluginId === effectivePluginId)) {
      continue;
    }

    const remoteWorkspaceId = readRemoteWorkspaceId(metadata);
    if (!remoteWorkspaceId) continue;

    links.push({
      pluginId: effectivePluginId,
      remoteWorkspaceId,
      syncEnabled: metadata.syncEnabled !== false,
    });
  }

  return links;
}

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

      if (
        ws.pluginMetadata?.[LEGACY_SYNC_PLUGIN_ID]
        && !ws.pluginMetadata?.[SYNC_PLUGIN_ID]
      ) {
        ws.pluginMetadata[SYNC_PLUGIN_ID] = ws.pluginMetadata[LEGACY_SYNC_PLUGIN_ID];
        delete ws.pluginMetadata[LEGACY_SYNC_PLUGIN_ID];
        needsSave = true;
      }

      if (ws.pluginMetadata) {
        for (const [pluginId, metadata] of Object.entries(ws.pluginMetadata)) {
          if (!metadata || typeof metadata !== 'object') continue;
          const remoteWorkspaceId = readRemoteWorkspaceId(metadata);
          if (!remoteWorkspaceId) continue;

          if (
            metadata.remoteWorkspaceId !== remoteWorkspaceId
            || metadata.serverId !== remoteWorkspaceId
          ) {
            ws.pluginMetadata[pluginId] = {
              ...metadata,
              remoteWorkspaceId,
              serverId: remoteWorkspaceId,
            };
            needsSave = true;
          }
        }
      }

      // Migration 2: server-UUID workspaces → local ID + provider link metadata
      // If a workspace has a non-local ID and isLocal === false, it was synced
      // with the old ID scheme. Give it a stable local ID and store the server
      // UUID in pluginMetadata.
      if (!ws.id.startsWith('local-') && !ws.isLocal) {
        const serverUuid = ws.id;
        const newLocalId = `local-${generateUUID()}`;

        // Store server UUID as a normalized provider link
        if (!ws.pluginMetadata) ws.pluginMetadata = {};
        ws.pluginMetadata[SYNC_PLUGIN_ID] = {
          ...(ws.pluginMetadata[SYNC_PLUGIN_ID] || {}),
          remoteWorkspaceId: serverUuid,
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
 * Check whether a workspace is linked to any remote provider.
 */
export function isWorkspaceSynced(id: string): boolean {
  const ws = registryState.find(w => w.id === id);
  return getWorkspaceProviderLinksFromWorkspace(ws).length > 0;
}

/**
 * Get the primary remote workspace ID for a linked workspace, or null if none.
 *
 * This preserves the legacy sync-oriented API shape for older callers.
 */
export function getServerWorkspaceId(id: string): string | null {
  return getPrimaryWorkspaceProviderLink(id)?.remoteWorkspaceId ?? null;
}

/**
 * Get all provider links stored on a workspace.
 */
export function getWorkspaceProviderLinks(id: string): WorkspaceProviderLink[] {
  const ws = registryState.find(w => w.id === id);
  return getWorkspaceProviderLinksFromWorkspace(ws);
}

/**
 * Get a provider-specific remote workspace link for a workspace.
 */
export function getWorkspaceProviderLink(
  id: string,
  pluginId: string,
): WorkspaceProviderLink | null {
  const effectivePluginId = normalizeProviderPluginId(pluginId);
  return getWorkspaceProviderLinks(id).find((entry) => entry.pluginId === effectivePluginId) ?? null;
}

/**
 * Get the primary provider link for a workspace.
 *
 * Hosts should use this when they only need "the linked remote workspace" and
 * don't care which provider supplied it.
 */
export function getPrimaryWorkspaceProviderLink(id: string): WorkspaceProviderLink | null {
  return getWorkspaceProviderLinks(id)[0] ?? null;
}

/**
 * Check whether live sync is enabled for a workspace's primary provider link.
 *
 * A workspace can be linked to a remote workspace for publish/bootstrap flows
 * without actively participating in live sync. In that case the remote ID
 * exists but `syncEnabled` is explicitly false.
 */
export function isWorkspaceSyncEnabled(id: string): boolean {
  const link = getPrimaryWorkspaceProviderLink(id);
  if (!link) return false;
  return link.syncEnabled;
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
 * Set remote-link status for a workspace via provider metadata.
 * When linked: stores the remote workspace ID in plugin metadata.
 * When unlinked: removes the provider metadata.
 * @deprecated Use setPluginMetadata() with the canonical plugin ID directly.
 */
export function setWorkspaceIsLocal(id: string, isLocal: boolean): void {
  if (isLocal) {
    // Remove sync metadata to mark as local-only
    setPluginMetadata(id, SYNC_PLUGIN_ID, null);
  }
  // If setting to linked (isLocal=false), the caller should also call
  // setPluginMetadata(id, providerId, { remoteWorkspaceId: ... }) to provide the remote ID.
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
  if (pluginId === LEGACY_SYNC_PLUGIN_ID || pluginId === SYNC_PLUGIN_ID) {
    return readSyncMetadata(ws);
  }
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
  const effectivePluginId =
    pluginId === LEGACY_SYNC_PLUGIN_ID ? SYNC_PLUGIN_ID : pluginId;

  if (data === null) {
    // Remove plugin metadata
    if (ws.pluginMetadata) {
      delete ws.pluginMetadata[effectivePluginId];
      if (effectivePluginId === SYNC_PLUGIN_ID) {
        delete ws.pluginMetadata[LEGACY_SYNC_PLUGIN_ID];
      }
      if (Object.keys(ws.pluginMetadata).length === 0) {
        delete ws.pluginMetadata;
      }
    }
  } else {
    // Merge plugin metadata
    if (!ws.pluginMetadata) {
      ws.pluginMetadata = {};
    }
    const nextMetadata = {
      ...(ws.pluginMetadata[effectivePluginId] || {}),
      ...data,
    };
    const remoteWorkspaceId = readRemoteWorkspaceId(nextMetadata);
    if (remoteWorkspaceId) {
      nextMetadata.remoteWorkspaceId = remoteWorkspaceId;
      nextMetadata.serverId = remoteWorkspaceId;
    }
    ws.pluginMetadata[effectivePluginId] = nextMetadata;
    if (effectivePluginId === SYNC_PLUGIN_ID) {
      delete ws.pluginMetadata[LEGACY_SYNC_PLUGIN_ID];
    }
  }

  // Keep isLocal in sync for backward compat
  ws.isLocal = getWorkspaceProviderLinksFromWorkspace(ws).length === 0;

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
 * Link a local workspace to a remote workspace by storing the remote ID
 * in provider metadata. The local workspace ID stays stable.
 *
 * @deprecated Use `setPluginMetadata()` with provider metadata directly.
 */
export function promoteLocalWorkspace(localId: string, serverUuid: string): void {
  setPluginMetadata(localId, SYNC_PLUGIN_ID, {
    remoteWorkspaceId: serverUuid,
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
    const knownIds = new Set(registryState.map(w => w.id));
    const knownNames = new Set(registryState.map(w => w.name));
    const discovered: LocalWorkspace[] = [];

    for await (const [name, handle] of (root as any).entries()) {
      if (handle.kind !== 'directory') continue;
      if (SYSTEM_DIRS.has(name)) continue;

      try {
        const diaryxDir = await handle.getDirectoryHandle('.diaryx', { create: false });
        const metadataFile = await diaryxDir.getFileHandle('workspace.json', { create: false });
        const metadata = JSON.parse(await (await metadataFile.getFile()).text()) as {
          id?: unknown;
          name?: unknown;
        };

        if (typeof metadata.id === 'string' && typeof metadata.name === 'string') {
          if (!knownIds.has(metadata.id)) {
            addLocalWorkspace({
              id: metadata.id,
              name: metadata.name,
              storageType: 'opfs',
            });
            const ws = getLocalWorkspace(metadata.id);
            if (ws) {
              discovered.push(ws);
              knownIds.add(metadata.id);
              knownNames.add(metadata.name);
              console.log(
                `[WorkspaceRegistry] Discovered workspace from metadata: ${metadata.name} (${metadata.id})`,
              );
            }
          }
          continue;
        }
      } catch {
        // No metadata — fall through to legacy name-based discovery.
      }

      if (knownNames.has(name) || name.startsWith('local-')) continue;

      const ws = createLocalWorkspace(name, 'opfs');
      discovered.push(ws);
      knownIds.add(ws.id);
      knownNames.add(name);
      console.log(`[WorkspaceRegistry] Discovered legacy OPFS workspace: ${name}`);
    }

    return discovered;
  } catch (e) {
    console.warn('[WorkspaceRegistry] Failed to discover OPFS workspaces:', e);
    return [];
  }
}
