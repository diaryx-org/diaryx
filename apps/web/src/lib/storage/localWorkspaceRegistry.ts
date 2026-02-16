/**
 * Local Workspace Registry — tracks workspaces available on this device.
 *
 * Stored in localStorage so it persists across sessions. Supports both:
 * - **Local-only workspaces** (`isLocal: true`): created client-side, no server backing
 * - **Synced workspaces** (`isLocal: false`): downloaded from a server, identified by server UUID
 */

// ============================================================================
// Types
// ============================================================================

export interface LocalWorkspace {
  /** Workspace identifier. Server UUID for synced workspaces, "local-<uuid>" for local-only. */
  id: string;
  /** Display name (also used as OPFS directory name) */
  name: string;
  /** Whether this workspace is local-only (not synced to server) */
  isLocal: boolean;
  /** When this workspace was first created/downloaded to this device */
  downloadedAt: number;
  /** When the user last opened this workspace */
  lastOpenedAt: number;
}

// ============================================================================
// Storage Keys
// ============================================================================

const REGISTRY_KEY = 'diaryx_local_workspaces';
const CURRENT_KEY = 'diaryx_current_workspace';

// ============================================================================
// Read Operations
// ============================================================================

/**
 * Get all locally available workspaces, sorted by last opened (most recent first).
 */
export function getLocalWorkspaces(): LocalWorkspace[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as LocalWorkspace[];
    // Migration: add isLocal field to entries that don't have it (default true = local)
    for (const ws of parsed) {
      if (ws.isLocal === undefined) {
        (ws as any).isLocal = true;
      }
    }
    return parsed.sort((a, b) => b.lastOpenedAt - a.lastOpenedAt);
  } catch {
    return [];
  }
}

/**
 * Get a single local workspace by ID.
 */
export function getLocalWorkspace(id: string): LocalWorkspace | null {
  return getLocalWorkspaces().find(w => w.id === id) ?? null;
}

/**
 * Get the currently active workspace ID, or null if none selected.
 */
export function getCurrentWorkspaceId(): string | null {
  if (typeof localStorage === 'undefined') return null;
  return localStorage.getItem(CURRENT_KEY);
}

/**
 * Check whether a workspace is downloaded locally.
 */
export function isWorkspaceLocal(id: string): boolean {
  return getLocalWorkspaces().some(w => w.id === id);
}

// ============================================================================
// Write Operations
// ============================================================================

/**
 * Register a workspace as locally available.
 * If it already exists, updates the name.
 */
export function addLocalWorkspace(ws: { id: string; name: string; isLocal?: boolean }): void {
  const list = getLocalWorkspaces();
  const existing = list.find(w => w.id === ws.id);
  if (existing) {
    existing.name = ws.name;
    if (ws.isLocal !== undefined) {
      existing.isLocal = ws.isLocal;
    }
  } else {
    list.push({
      id: ws.id,
      name: ws.name,
      isLocal: ws.isLocal ?? false,
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
  const list = getLocalWorkspaces().filter(w => w.id !== id);
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
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(CURRENT_KEY, id);

  // Update lastOpenedAt and keep workspace name in sync
  const list = getLocalWorkspaces();
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.lastOpenedAt = Date.now();
    saveRegistry(list);
    // Keep localStorage workspace name in sync for page reloads
    localStorage.setItem('diaryx-workspace-name', ws.name);
  }
}

/**
 * Clear the current workspace selection.
 */
export function clearCurrentWorkspaceId(): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.removeItem(CURRENT_KEY);
}

/**
 * Rename a workspace in the local registry.
 */
export function renameLocalWorkspace(id: string, newName: string): void {
  const list = getLocalWorkspaces();
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
 * Set whether a workspace is local-only or synced.
 * When set to true, the workspace stops syncing but local data is preserved.
 * When set to false, the workspace is eligible for syncing.
 */
export function setWorkspaceIsLocal(id: string, isLocal: boolean): void {
  const list = getLocalWorkspaces();
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.isLocal = isLocal;
    saveRegistry(list);
  }
}

// ============================================================================
// Local Workspace Operations
// ============================================================================

/**
 * Create a new local-only workspace (no server required).
 * Returns the created workspace entry.
 */
export function createLocalWorkspace(name: string): LocalWorkspace {
  const id = `local-${crypto.randomUUID()}`;
  const ws: LocalWorkspace = {
    id,
    name,
    isLocal: true,
    downloadedAt: Date.now(),
    lastOpenedAt: Date.now(),
  };
  const list = getLocalWorkspaces();
  list.push(ws);
  saveRegistry(list);
  return ws;
}

/**
 * Promote a local-only workspace to a synced workspace.
 * Updates the ID from "local-xxx" to the server UUID and sets isLocal to false.
 * The OPFS directory name stays the same (name-based).
 */
export function promoteLocalWorkspace(localId: string, serverUuid: string): void {
  const list = getLocalWorkspaces();
  const ws = list.find(w => w.id === localId);
  if (ws) {
    ws.id = serverUuid;
    ws.isLocal = false;
    saveRegistry(list);
    // Update current workspace ID if this was the active one
    if (getCurrentWorkspaceId() === localId) {
      localStorage.setItem(CURRENT_KEY, serverUuid);
    }
  }
}

/**
 * Bootstrap the registry for first-time users.
 * If the registry is empty and a "My Journal" OPFS dir exists (or we're in a fresh state),
 * creates a default local-only workspace entry.
 */
export function bootstrapDefaultWorkspace(): LocalWorkspace {
  const list = getLocalWorkspaces();
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
    const knownNames = new Set(getLocalWorkspaces().map(w => w.name));
    const discovered: LocalWorkspace[] = [];

    for await (const [name, handle] of (root as any).entries()) {
      if (handle.kind !== 'directory') continue;
      if (SYSTEM_DIRS.has(name)) continue;
      if (knownNames.has(name)) continue;

      // This is an unregistered workspace directory — add it
      const ws = createLocalWorkspace(name);
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

// ============================================================================
// Helpers
// ============================================================================

function saveRegistry(list: LocalWorkspace[]): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(REGISTRY_KEY, JSON.stringify(list));
}
