/**
 * Collaboration Store - Manages Y.js collaboration state
 *
 * This store holds state related to real-time collaboration,
 * including Y.Doc, provider, connection status, and server configuration.
 */

import type { Doc as YDoc } from 'yjs';
import type { HocuspocusProvider } from '@hocuspocus/provider';

// ============================================================================
// State
// ============================================================================

// Y.js document and provider
let currentYDoc = $state<YDoc | null>(null);
let currentProvider = $state<HocuspocusProvider | null>(null);
let currentCollaborationPath = $state<string | null>(null);

// Connection status
let collaborationEnabled = $state(false);
let collaborationConnected = $state(false);

// Sync status for multi-device sync
export type SyncStatus = 'not_configured' | 'idle' | 'connecting' | 'syncing' | 'synced' | 'error';
let syncStatus = $state<SyncStatus>('not_configured');
let syncProgress = $state<{ total: number; completed: number } | null>(null);
let syncError = $state<string | null>(null);

// Body sync status (tracked separately from metadata sync)
export type BodySyncStatus = 'idle' | 'syncing' | 'synced';
let bodySyncStatus = $state<BodySyncStatus>('idle');
let bodySyncProgress = $state<{ total: number; completed: number } | null>(null);

// Server configuration
function getInitialServerUrl(): string | null {
  if (typeof window !== 'undefined') {
    const saved = localStorage.getItem('diaryx-sync-server');
    if (saved) return saved;
  }
  if (typeof import.meta !== 'undefined' && (import.meta as any).env?.VITE_COLLAB_SERVER) {
    return (import.meta as any).env.VITE_COLLAB_SERVER;
  }
  return null;
}

let collaborationServerUrl = $state<string | null>(getInitialServerUrl());

// ============================================================================
// Store Factory
// ============================================================================

/**
 * Get the collaboration store singleton.
 */
export function getCollaborationStore() {
  return {
    // Getters
    get currentYDoc() { return currentYDoc; },
    get currentProvider() { return currentProvider; },
    get currentCollaborationPath() { return currentCollaborationPath; },
    get collaborationEnabled() { return collaborationEnabled; },
    get collaborationConnected() { return collaborationConnected; },
    get collaborationServerUrl() { return collaborationServerUrl; },
    get syncStatus() { return syncStatus; },
    get syncProgress() { return syncProgress; },
    get syncError() { return syncError; },
    get bodySyncStatus() { return bodySyncStatus; },
    get bodySyncProgress() { return bodySyncProgress; },
    /**
     * Effective sync status that only shows 'synced' when BOTH metadata AND body are synced.
     * Use this in UI components to accurately represent sync completion.
     */
    get effectiveSyncStatus(): SyncStatus {
      // If there's an error, show error
      if (syncStatus === 'error') return 'error';
      // If not configured, show not configured
      if (syncStatus === 'not_configured') return 'not_configured';
      // If either is connecting, show connecting
      if (syncStatus === 'connecting') return 'connecting';
      // If either is syncing, show syncing
      if (syncStatus === 'syncing' || bodySyncStatus === 'syncing') return 'syncing';
      // Only show synced if both metadata and body are synced
      if (syncStatus === 'synced' && bodySyncStatus === 'synced') return 'synced';
      // Otherwise show idle (metadata synced but body not yet synced)
      if (syncStatus === 'synced' && bodySyncStatus === 'idle') return 'syncing';
      return syncStatus;
    },

    // Y.Doc management
    setYDoc(ydoc: YDoc | null) {
      currentYDoc = ydoc;
    },

    setProvider(provider: HocuspocusProvider | null) {
      currentProvider = provider;
    },

    setCollaborationPath(path: string | null) {
      currentCollaborationPath = path;
    },

    // Set all collaboration state at once
    setCollaborationSession(
      ydoc: YDoc | null,
      provider: HocuspocusProvider | null,
      path: string | null
    ) {
      currentYDoc = ydoc;
      currentProvider = provider;
      currentCollaborationPath = path;
    },

    // Clear collaboration session
    clearCollaborationSession() {
      currentYDoc = null;
      currentProvider = null;
      currentCollaborationPath = null;
    },

    // Connection status
    setEnabled(enabled: boolean) {
      collaborationEnabled = enabled;
    },

    setConnected(connected: boolean) {
      collaborationConnected = connected;
    },

    // Sync status for multi-device sync
    setSyncStatus(status: SyncStatus) {
      syncStatus = status;
      // Clear error when status changes to non-error state
      if (status !== 'error') {
        syncError = null;
      }
    },

    setSyncProgress(progress: { total: number; completed: number } | null) {
      syncProgress = progress;
    },

    setSyncError(error: string | null | unknown) {
      // Defensive string conversion - error might be an object from Rust
      if (error === null || error === undefined) {
        syncError = null;
      } else if (typeof error === 'string') {
        syncError = error;
      } else if (error instanceof Error) {
        syncError = error.message;
      } else if (typeof error === 'object') {
        // Try to extract message from object, fallback to JSON stringify
        const errObj = error as Record<string, unknown>;
        if (typeof errObj.message === 'string') {
          syncError = errObj.message;
        } else if (typeof errObj.error === 'string') {
          syncError = errObj.error;
        } else {
          try {
            syncError = JSON.stringify(error);
          } catch {
            syncError = 'Unknown error';
          }
        }
      } else {
        syncError = String(error);
      }
      if (syncError) {
        syncStatus = 'error';
      }
    },

    // Body sync status methods
    setBodySyncStatus(status: BodySyncStatus) {
      bodySyncStatus = status;
    },

    setBodySyncProgress(progress: { total: number; completed: number } | null) {
      bodySyncProgress = progress;
      // Auto-update body sync status based on progress
      if (progress) {
        if (progress.completed < progress.total) {
          bodySyncStatus = 'syncing';
        } else {
          bodySyncStatus = 'synced';
        }
      }
    },

    /**
     * Reset body sync status to idle.
     * Call when starting a new sync session or when body sync is not needed.
     */
    resetBodySyncStatus() {
      bodySyncStatus = 'idle';
      bodySyncProgress = null;
    },

    // Server URL
    setServerUrl(url: string | null) {
      collaborationServerUrl = url;
      if (typeof window !== 'undefined') {
        if (url) {
          localStorage.setItem('diaryx-sync-server', url);
        } else {
          localStorage.removeItem('diaryx-sync-server');
        }
      }
    },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const collaborationStore = getCollaborationStore();
