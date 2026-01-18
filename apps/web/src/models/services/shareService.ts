/**
 * Share Service - Manages live collaboration session lifecycle
 *
 * This service handles:
 * - Creating share sessions (host mode)
 * - Joining share sessions (guest mode)
 * - Guest backend management (in-memory filesystem for web guests)
 * - Session cleanup
 */

import { shareSessionStore } from '../stores/shareSessionStore.svelte';
import { collaborationStore } from '../stores/collaborationStore.svelte';
import { workspaceStore } from '../stores/workspaceStore.svelte';
import { startSessionSync, stopSessionSync, setBackendApi } from '$lib/crdt/workspaceCrdtBridge';
import { setActiveSessionCode } from '$lib/crdt/collaborationBridge';
import { createGuestBackend, type WorkerBackendNew } from '$lib/backend/workerBackendNew';
import { createApi, type Api } from '$lib/backend/api';
import { isTauri, type Backend } from '$lib/backend/interface';
import type { TauriBackend } from '$lib/backend/tauri';

// ============================================================================
// Types
// ============================================================================

export interface SessionCreatedResponse {
  type: 'session_created';
  joinCode: string;
  workspaceId: string;
  readOnly: boolean;
}

export interface SessionJoinedResponse {
  type: 'session_joined';
  joinCode: string;
  workspaceId: string;
  readOnly: boolean;
}

export interface ReadOnlyChangedMessage {
  type: 'read_only_changed';
  readOnly: boolean;
}

export interface PeerJoinedMessage {
  type: 'peer_joined';
  guestId: string;
  peerCount: number;
}

export interface PeerLeftMessage {
  type: 'peer_left';
  guestId: string;
  peerCount: number;
}

export interface ErrorResponse {
  type: 'error';
  message: string;
}

type ServerMessage = SessionCreatedResponse | SessionJoinedResponse | PeerJoinedMessage | PeerLeftMessage | ReadOnlyChangedMessage | ErrorResponse;

// ============================================================================
// Constants
// ============================================================================

const GUEST_STORAGE_PREFIX = 'guest';
const CONNECTION_TIMEOUT = 10000; // 10 seconds

// ============================================================================
// State
// ============================================================================

let currentServerUrl: string | null = null;

// Guest backend and API (for web guests using in-memory storage)
let guestBackend: WorkerBackendNew | null = null;
let guestApi: Api | null = null;

// Original backend (saved before switching to guest backend)
let originalBackend: Backend | null = null;

// ============================================================================
// Helpers
// ============================================================================

function getServerUrl(): string {
  // Use collaboration server URL from store, or default
  const storeUrl = collaborationStore.collaborationServerUrl;
  if (storeUrl) {
    // Convert http(s) to ws(s)
    return storeUrl.replace(/^http/, 'ws');
  }
  // Default to localhost for development
  return 'wss://sync.diaryx.org';
}

function validateJoinCode(code: string): boolean {
  return /^[A-Z0-9]{8}-[A-Z0-9]{8}$/i.test(code);
}

function normalizeJoinCode(code: string): string {
  return code.toUpperCase().trim();
}

// ============================================================================
// Session Creation (Host Mode)
// ============================================================================

// Owner ID for this client (used for read-only enforcement)
const clientOwnerId = `owner-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

/**
 * Create a new share session for the current workspace.
 * Returns the join code that others can use to join.
 */
export async function createShareSession(workspaceId: string, readOnly: boolean = false, audience: string | null = null): Promise<string> {
  // Store audience for later reference (set before startHosting)
  const selectedAudience = audience;
  const serverUrl = getServerUrl();
  currentServerUrl = serverUrl;

  shareSessionStore.setConnecting(true);

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.close();
      shareSessionStore.setError('Connection timeout');
      reject(new Error('Connection timeout'));
    }, CONNECTION_TIMEOUT);

    const wsUrl = `${serverUrl}?action=create&workspaceId=${encodeURIComponent(workspaceId)}&ownerId=${encodeURIComponent(clientOwnerId)}&readOnly=${readOnly}`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      console.log('[ShareService] Connected to server for session creation');
    };

    ws.onmessage = (event) => {
      try {
        const msg: ServerMessage = JSON.parse(event.data);

        if (msg.type === 'session_created') {
          clearTimeout(timeout);
          shareSessionStore.startHosting(msg.joinCode, msg.workspaceId, msg.readOnly, selectedAudience);
          shareSessionStore.setSessionWs(ws);
          console.log('[ShareService] Session created:', msg.joinCode, 'readOnly:', msg.readOnly, 'audience:', selectedAudience);

          // Start document sync for this session (isHost=true to send initial state)
          const syncServerUrl = currentServerUrl!;
          startSessionSync(syncServerUrl, msg.joinCode, true);

          // Set active session code for per-document sync (real-time editing)
          setActiveSessionCode(msg.joinCode);

          resolve(msg.joinCode);
        } else if (msg.type === 'peer_joined') {
          shareSessionStore.addPeer(msg.guestId);
          console.log('[ShareService] Peer joined:', msg.guestId);
        } else if (msg.type === 'peer_left') {
          shareSessionStore.removePeer(msg.guestId);
          console.log('[ShareService] Peer left:', msg.guestId);
        } else if (msg.type === 'read_only_changed') {
          shareSessionStore.setReadOnly(msg.readOnly);
          console.log('[ShareService] Read-only changed:', msg.readOnly);
        } else if (msg.type === 'error') {
          clearTimeout(timeout);
          shareSessionStore.setError(msg.message);
          ws.close();
          reject(new Error(msg.message));
        }
      } catch (e) {
        console.error('[ShareService] Failed to parse message:', e);
      }
    };

    ws.onerror = (event) => {
      clearTimeout(timeout);
      console.error('[ShareService] WebSocket error:', event);
      shareSessionStore.setError('Connection failed');
      reject(new Error('Connection failed'));
    };

    ws.onclose = () => {
      clearTimeout(timeout);
      if (shareSessionStore.mode === 'hosting' && shareSessionStore.connected) {
        // Unexpected disconnect while hosting
        shareSessionStore.setConnected(false);
        shareSessionStore.setError('Disconnected from server');
      }
    };
  });
}

// ============================================================================
// Session Join (Guest Mode)
// ============================================================================

/**
 * Join an existing share session using a join code.
 * Returns the workspace ID of the session.
 *
 * For web guests: Creates an in-memory backend (files stored in memory only).
 * For Tauri guests: Uses OPFS with path prefixing (deferred to future PR).
 */
export async function joinShareSession(joinCode: string): Promise<string> {
  const normalizedCode = normalizeJoinCode(joinCode);

  if (!validateJoinCode(normalizedCode)) {
    throw new Error('Invalid join code format');
  }

  const serverUrl = getServerUrl();
  currentServerUrl = serverUrl;

  shareSessionStore.setConnecting(true);

  // For guests, create an in-memory backend (Tauri or web)
  if (isTauri()) {
    // Tauri guest: use Rust-side in-memory filesystem
    console.log('[ShareService] Starting Tauri guest mode...');
    try {
      const backend = workspaceStore.backend as TauriBackend;
      await backend.startGuestMode(normalizedCode);

      // Save tree state so we can restore when leaving
      workspaceStore.saveTreeState();

      // Clear the tree - it will be populated from CRDT sync
      workspaceStore.setTree(null);

      console.log('[ShareService] Tauri guest mode ready');
    } catch (e) {
      console.error('[ShareService] Failed to start Tauri guest mode:', e);
      shareSessionStore.setError('Failed to initialize guest mode');
      throw e;
    }
  } else {
    // Web guest: use JavaScript-side in-memory backend
    console.log('[ShareService] Creating in-memory backend for web guest...');
    try {
      // Save the original backend and tree state so we can restore when leaving
      originalBackend = workspaceStore.backend;
      workspaceStore.saveTreeState();

      // Create the in-memory guest backend
      guestBackend = await createGuestBackend();
      guestApi = createApi(guestBackend);

      // Set the guest backend in workspaceStore so App.svelte uses it
      workspaceStore.setBackend(guestBackend);

      // Clear the tree - it will be populated from CRDT sync
      workspaceStore.setTree(null);

      // Set the guest API for the CRDT bridge to use for file operations
      setBackendApi(guestApi);

      console.log('[ShareService] In-memory guest backend ready');
    } catch (e) {
      console.error('[ShareService] Failed to create guest backend:', e);
      shareSessionStore.setError('Failed to initialize guest storage');
      throw e;
    }
  }

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      ws.close();
      shareSessionStore.setError('Connection timeout');
      reject(new Error('Connection timeout'));
    }, CONNECTION_TIMEOUT);

    const guestId = `guest-${Date.now()}`;
    const wsUrl = `${serverUrl}?action=join&code=${encodeURIComponent(normalizedCode)}&guestId=${encodeURIComponent(guestId)}`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      console.log('[ShareService] Connected to server for session join');
    };

    ws.onmessage = (event) => {
      try {
        const msg: ServerMessage = JSON.parse(event.data);

        if (msg.type === 'session_joined') {
          clearTimeout(timeout);
          // Both Tauri and web now use in-memory storage for guest sessions
          const backendType = 'memory';
          shareSessionStore.startGuest(msg.joinCode, msg.workspaceId, undefined, backendType, msg.readOnly);
          shareSessionStore.setSessionWs(ws);
          console.log('[ShareService] Joined session:', msg.joinCode, 'backendType:', backendType, 'readOnly:', msg.readOnly);

          // Start document sync for this session (isHost=false, receive state from server)
          const syncServerUrl = currentServerUrl!;
          startSessionSync(syncServerUrl, msg.joinCode, false);

          // Set active session code for per-document sync (real-time editing)
          setActiveSessionCode(msg.joinCode);

          resolve(msg.workspaceId);
        } else if (msg.type === 'read_only_changed') {
          shareSessionStore.setReadOnly(msg.readOnly);
          console.log('[ShareService] Read-only changed:', msg.readOnly);
        } else if (msg.type === 'error') {
          clearTimeout(timeout);
          shareSessionStore.setError(msg.message);
          ws.close();
          reject(new Error(msg.message));
        }
      } catch (e) {
        console.error('[ShareService] Failed to parse message:', e);
      }
    };

    ws.onerror = (event) => {
      clearTimeout(timeout);
      console.error('[ShareService] WebSocket error:', event);
      shareSessionStore.setError('Connection failed');
      reject(new Error('Connection failed'));
    };

    ws.onclose = () => {
      clearTimeout(timeout);
      if (shareSessionStore.mode === 'guest' && shareSessionStore.connected) {
        // Unexpected disconnect while guest
        shareSessionStore.setConnected(false);
        shareSessionStore.setError('Disconnected from session');
      }
    };
  });
}

// ============================================================================
// Session End
// ============================================================================

/**
 * End the current share session (works for both host and guest).
 */
export async function endShareSession(): Promise<void> {
  console.log('[ShareService] Ending share session');

  // Capture state before endSession() clears it
  const joinCode = shareSessionStore.joinCode;
  const wasGuest = shareSessionStore.isGuest;
  const usedInMemory = shareSessionStore.usesInMemoryStorage;

  // Stop document sync
  stopSessionSync();

  // Clear active session code for per-document sync
  setActiveSessionCode(null);

  // End the session (clears state)
  shareSessionStore.endSession();

  // Clean up guest resources
  if (wasGuest) {
    if (isTauri()) {
      // For Tauri guests: end guest mode in Rust backend
      console.log('[ShareService] Ending Tauri guest mode...');
      try {
        const backend = workspaceStore.backend as TauriBackend;
        await backend.endGuestMode();

        // Restore the original tree state
        workspaceStore.restoreTreeState();

        console.log('[ShareService] Tauri guest mode ended');
      } catch (e) {
        console.error('[ShareService] Failed to end Tauri guest mode:', e);
      }
    } else if (usedInMemory) {
      // For web in-memory guests: restore the original backend and clear guest references
      console.log('[ShareService] Cleaning up in-memory guest backend...');

      // Restore the original backend in workspaceStore
      if (originalBackend) {
        workspaceStore.setBackend(originalBackend);
        // Also restore the original API for the CRDT bridge
        setBackendApi(createApi(originalBackend));
        console.log('[ShareService] Restored original backend');
      }

      // Restore the original tree state
      workspaceStore.restoreTreeState();

      // Clear guest references (memory freed when garbage collected)
      guestBackend = null;
      guestApi = null;
      originalBackend = null;
    } else if (joinCode) {
      // For OPFS guests: clean up the OPFS storage
      console.log('[ShareService] Cleaning up OPFS guest storage...');
      await cleanupGuestStorage(joinCode);
    }
  }
}

// ============================================================================
// Session Control
// ============================================================================

/**
 * Toggle read-only mode for the current session (host only).
 * @param readOnly - Whether the session should be read-only
 */
export function setSessionReadOnly(readOnly: boolean): void {
  if (!shareSessionStore.isHosting) {
    console.warn('[ShareService] setSessionReadOnly called but not hosting');
    return;
  }

  const ws = shareSessionStore.sessionWs;
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    console.warn('[ShareService] setSessionReadOnly called but WebSocket not open');
    return;
  }

  // Send message to server to toggle read-only
  ws.send(JSON.stringify({
    type: 'set_read_only',
    readOnly,
  }));

  console.log('[ShareService] Requested read-only change:', readOnly);
}

/**
 * Get the owner ID for this client.
 * Used for read-only enforcement in document sync.
 */
export function getClientOwnerId(): string {
  return clientOwnerId;
}

// ============================================================================
// Guest Backend Access
// ============================================================================

/**
 * Get the guest backend (for web guests using in-memory storage).
 * Returns null if not in guest mode or if using Tauri.
 */
export function getGuestBackend(): WorkerBackendNew | null {
  return guestBackend;
}

/**
 * Get the guest API wrapper (for web guests using in-memory storage).
 * Returns null if not in guest mode or if using Tauri.
 */
export function getGuestApi(): Api | null {
  return guestApi;
}

// ============================================================================
// Guest Storage Management (OPFS - for Tauri guests, future)
// ============================================================================

/**
 * Get the OPFS storage path for a guest session.
 * Only used for Tauri guests (future) - web guests use in-memory storage.
 */
export function getGuestStoragePath(joinCode: string): string {
  return `/${GUEST_STORAGE_PREFIX}/${joinCode}`;
}

/**
 * Check if we're currently in guest mode.
 */
export function isGuestMode(): boolean {
  return shareSessionStore.isGuest;
}

/**
 * Get the current session's join code.
 */
export function getCurrentJoinCode(): string | null {
  return shareSessionStore.joinCode;
}

/**
 * Get the server URL for document sync within the current session.
 */
export function getSessionSyncUrl(docName: string): string | null {
  if (!currentServerUrl || !shareSessionStore.joinCode) {
    return null;
  }
  return `${currentServerUrl}?doc=${encodeURIComponent(docName)}&session=${encodeURIComponent(shareSessionStore.joinCode)}`;
}

// ============================================================================
// Cleanup
// ============================================================================

/**
 * Clean up guest storage for a specific session.
 * This should be called when leaving a guest session.
 */
export async function cleanupGuestStorage(joinCode: string): Promise<void> {
  try {
    // Get OPFS root
    const root = await navigator.storage.getDirectory();

    // Try to get the guest folder
    try {
      const guestFolder = await root.getDirectoryHandle(GUEST_STORAGE_PREFIX);

      // Try to remove the session folder
      try {
        await guestFolder.removeEntry(joinCode, { recursive: true });
        console.log(`[ShareService] Cleaned up guest storage for session: ${joinCode}`);
      } catch {
        // Session folder doesn't exist, that's fine
      }

      // Check if guest folder is now empty
      let hasEntries = false;
      // Type assertion needed as TypeScript types may not include keys()
      for await (const _ of (guestFolder as any).keys()) {
        hasEntries = true;
        break;
      }

      // If empty, remove the guest folder too
      if (!hasEntries) {
        await root.removeEntry(GUEST_STORAGE_PREFIX);
        console.log('[ShareService] Removed empty guest folder');
      }
    } catch {
      // Guest folder doesn't exist, that's fine
    }
  } catch (e) {
    console.error('[ShareService] Failed to cleanup guest storage:', e);
  }
}

/**
 * Clean up all guest storage (used during app reset/cleanup).
 */
export async function cleanupAllGuestStorage(): Promise<void> {
  try {
    const root = await navigator.storage.getDirectory();
    try {
      await root.removeEntry(GUEST_STORAGE_PREFIX, { recursive: true });
      console.log('[ShareService] Cleaned up all guest storage');
    } catch {
      // Guest folder doesn't exist, that's fine
    }
  } catch (e) {
    console.error('[ShareService] Failed to cleanup all guest storage:', e);
  }
}
