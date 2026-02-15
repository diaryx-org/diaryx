/**
 * Local Workspace Registry — tracks which server workspaces are downloaded locally.
 *
 * Stored in localStorage so it persists across sessions. The server is the source
 * of truth for which workspaces *exist*; this registry only tracks which ones
 * are available offline on this device.
 */

// ============================================================================
// Types
// ============================================================================

export interface LocalWorkspace {
  /** Server workspace UUID */
  id: string;
  /** Display name */
  name: string;
  /** When this workspace was first downloaded to this device */
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
 * Get all locally downloaded workspaces, sorted by last opened (most recent first).
 */
export function getLocalWorkspaces(): LocalWorkspace[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(REGISTRY_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as LocalWorkspace[];
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
 * Register a workspace as locally downloaded.
 * If it already exists, updates the name.
 */
export function addLocalWorkspace(ws: { id: string; name: string }): void {
  const list = getLocalWorkspaces();
  const existing = list.find(w => w.id === ws.id);
  if (existing) {
    existing.name = ws.name;
  } else {
    list.push({
      id: ws.id,
      name: ws.name,
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
 * Also updates `lastOpenedAt`.
 */
export function setCurrentWorkspaceId(id: string): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(CURRENT_KEY, id);

  // Update lastOpenedAt
  const list = getLocalWorkspaces();
  const ws = list.find(w => w.id === id);
  if (ws) {
    ws.lastOpenedAt = Date.now();
    saveRegistry(list);
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
  }
}

// ============================================================================
// Helpers
// ============================================================================

function saveRegistry(list: LocalWorkspace[]): void {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(REGISTRY_KEY, JSON.stringify(list));
}
