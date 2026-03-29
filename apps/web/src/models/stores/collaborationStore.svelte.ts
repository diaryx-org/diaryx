/**
 * Collaboration Store - Manages Y.js collaboration state
 *
 * This store holds state related to real-time collaboration,
 * including Y.Doc, provider, connection status, and server configuration.
 */


// ============================================================================
// Types
// ============================================================================

export type SyncStatus = 'not_configured' | 'idle' | 'connecting' | 'syncing' | 'synced' | 'error';
export type BodySyncStatus = 'idle' | 'syncing' | 'synced';

// ============================================================================
// Helpers
// ============================================================================

function getInitialServerUrl(): string | null {
  if (typeof window !== 'undefined') {
    // Check canonical key first (used by auth store / onboarding flows)
    const canonical = localStorage.getItem('diaryx_sync_server_url');
    if (canonical) return canonical;
    // Fall back to legacy key for backwards compatibility
    const legacy = localStorage.getItem('diaryx-sync-server');
    if (legacy) return legacy;
  }
  if (typeof import.meta !== 'undefined' && (import.meta as any).env?.VITE_COLLAB_SERVER) {
    return (import.meta as any).env.VITE_COLLAB_SERVER;
  }
  return null;
}

function toReadableErrorMessage(error: unknown, depth = 0): string | null {
  if (error === null || error === undefined) return null;
  if (depth > 5) return null;

  if (typeof error === 'string') {
    const trimmed = error.trim();
    if (!trimmed) return null;
    if (trimmed === '[object Object]') return null;

    // Some callers pass stringified objects; try to recover the useful message.
    if ((trimmed.startsWith('{') || trimmed.startsWith('[')) && trimmed.length < 20_000) {
      try {
        const parsed = JSON.parse(trimmed);
        const parsedMessage = toReadableErrorMessage(parsed, depth + 1);
        if (parsedMessage) return parsedMessage;
      } catch {
        // Fall through and use the original string.
      }
    }

    return trimmed;
  }

  if (error instanceof Error) {
    return error.message?.trim() || error.name || null;
  }

  if (typeof error === 'object') {
    if (Array.isArray(error)) {
      for (const entry of error) {
        const entryMessage = toReadableErrorMessage(entry, depth + 1);
        if (entryMessage) return entryMessage;
      }
      return null;
    }

    const errObj = error as Record<string, unknown>;
    for (const key of ['message', 'error', 'detail', 'reason', 'description', 'cause']) {
      const value = errObj[key];
      const message = toReadableErrorMessage(value, depth + 1);
      if (message) return message;
    }

    try {
      const serialized = JSON.stringify(error);
      if (serialized && serialized !== '{}' && serialized !== '[]') {
        return serialized;
      }
    } catch {
      // Ignore and fall through.
    }
    return null;
  }

  const primitive = String(error).trim();
  return primitive || null;
}

// ============================================================================
// Store
// ============================================================================

class CollaborationStore {

  currentCollaborationPath = $state<string | null>(null);

  // Connection status
  collaborationEnabled = $state(false);
  collaborationConnected = $state(false);

  // Sync status for multi-device sync
  syncStatus = $state<SyncStatus>('not_configured');
  syncProgress = $state<{ total: number; completed: number } | null>(null);
  syncError = $state<string | null>(null);

  // Body sync status (tracked separately from metadata sync)
  bodySyncStatus = $state<BodySyncStatus>('idle');
  bodySyncProgress = $state<{ total: number; completed: number } | null>(null);

  // Server configuration
  collaborationServerUrl = $state<string | null>(getInitialServerUrl());

  /**
   * When true, the sync server was unreachable at startup.
   * All sync requests should be suppressed until the user triggers a reconnect.
   */
  serverOffline = $state(false);

  /**
   * Effective sync status that only shows 'synced' when BOTH metadata AND body are synced.
   * Use this in UI components to accurately represent sync completion.
   *
   * Uses $derived.by instead of a getter so Svelte 5 creates a proper signal node
   * in the reactive graph, ensuring reliable UI updates when dependencies change.
   */
  effectiveSyncStatus: SyncStatus = $derived.by(() => {
    // If server is offline, show error
    if (this.serverOffline) return 'error';
    // If there's an error, show error
    if (this.syncStatus === 'error') return 'error';
    // If not configured, show not configured
    if (this.syncStatus === 'not_configured') return 'not_configured';
    // If either is connecting, show connecting
    if (this.syncStatus === 'connecting') return 'connecting';
    // If either is syncing, show syncing
    if (this.syncStatus === 'syncing' || this.bodySyncStatus === 'syncing') return 'syncing';
    // Only show synced if both metadata and body are synced
    if (this.syncStatus === 'synced' && this.bodySyncStatus === 'synced') return 'synced';
    // Otherwise show idle (metadata synced but body not yet synced)
    if (this.syncStatus === 'synced' && this.bodySyncStatus === 'idle') return 'syncing';
    return this.syncStatus;
  });


  setCollaborationPath(path: string | null) {
    this.currentCollaborationPath = path;
  }

  // Set all collaboration state at once
  setCollaborationSession(path: string | null) {
    this.currentCollaborationPath = path;
  }

  // Clear collaboration session
  clearCollaborationSession() {
    this.currentCollaborationPath = null;
  }

  // Connection status
  setEnabled(enabled: boolean) {
    this.collaborationEnabled = enabled;
  }

  setConnected(connected: boolean) {
    this.collaborationConnected = connected;
  }

  // Sync status for multi-device sync
  setSyncStatus(status: SyncStatus) {
    this.syncStatus = status;
    // Clear error when status changes to non-error state
    if (status !== 'error') {
      this.syncError = null;
    }
  }

  setSyncProgress(progress: { total: number; completed: number } | null) {
    this.syncProgress = progress;
  }

  setSyncError(error: string | null | unknown) {
    if (error === null || error === undefined) {
      this.syncError = null;
      return;
    }

    const readable = toReadableErrorMessage(error);
    this.syncError = readable ?? 'Unexpected sync error';
    if (this.syncError) {
      this.syncStatus = 'error';
    }
  }

  // Body sync status methods
  setBodySyncStatus(status: BodySyncStatus) {
    this.bodySyncStatus = status;
  }

  setBodySyncProgress(progress: { total: number; completed: number } | null) {
    this.bodySyncProgress = progress;
    // Auto-update body sync status based on progress
    if (progress) {
      if (progress.completed < progress.total) {
        this.bodySyncStatus = 'syncing';
      } else {
        this.bodySyncStatus = 'synced';
      }
    }
  }

  /**
   * Reset body sync status to idle.
   * Call when starting a new sync session or when body sync is not needed.
   */
  resetBodySyncStatus() {
    this.bodySyncStatus = 'idle';
    this.bodySyncProgress = null;
  }

  setServerOffline(offline: boolean) {
    this.serverOffline = offline;
    if (offline) {
      this.syncError = 'Server unreachable — working offline';
    }
  }

  // Server URL (in-memory only — the canonical localStorage key
  // `diaryx_sync_server_url` is managed by authStore.setServerUrl()
  // which always stores the HTTP URL.  We must NOT overwrite it here
  // because collaborationStore receives ws:// URLs.)
  setServerUrl(url: string | null) {
    this.collaborationServerUrl = url;
  }
}

// ============================================================================
// Convenience export
// ============================================================================

export const collaborationStore = new CollaborationStore();

/**
 * Get the collaboration store singleton.
 */
export function getCollaborationStore() {
  return collaborationStore;
}
