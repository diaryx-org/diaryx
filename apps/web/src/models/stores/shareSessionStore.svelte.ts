/**
 * Share Session Store - Manages live collaboration sharing state
 *
 * This store tracks whether the user is hosting or joining a share session,
 * the join code, connection status, and peer information.
 */

// ============================================================================
// Types
// ============================================================================

export type ShareMode = 'idle' | 'hosting' | 'guest';

export interface PeerInfo {
  id: string;
  name?: string;
  joinedAt: Date;
}

// ============================================================================
// State
// ============================================================================

// Current mode
let mode = $state<ShareMode>('idle');

// Session info
let joinCode = $state<string | null>(null);
let workspaceId = $state<string | null>(null);

// Connection status
let connected = $state(false);
let connecting = $state(false);
let error = $state<string | null>(null);

// Peer tracking (for host mode)
let peerCount = $state(0);
let peers = $state<PeerInfo[]>([]);

// Guest mode info
let hostName = $state<string | null>(null);

// WebSocket connection for session management
let sessionWs = $state<WebSocket | null>(null);

// Session options
let readOnly = $state(false);
let audience = $state<string | null>(null);

// Guest backend type - 'memory' means using in-memory FS (no path prefixing needed)
let guestBackendType = $state<'memory' | 'opfs' | null>(null);

// ============================================================================
// Store Factory
// ============================================================================

export function getShareSessionStore() {
  return {
    // Getters
    get mode() { return mode; },
    get joinCode() { return joinCode; },
    get workspaceId() { return workspaceId; },
    get connected() { return connected; },
    get connecting() { return connecting; },
    get error() { return error; },
    get peerCount() { return peerCount; },
    get peers() { return peers; },
    get hostName() { return hostName; },
    get sessionWs() { return sessionWs; },
    get guestBackendType() { return guestBackendType; },
    get readOnly() { return readOnly; },
    get audience() { return audience; },
    get isHosting() { return mode === 'hosting'; },
    get isGuest() { return mode === 'guest'; },
    get isIdle() { return mode === 'idle'; },
    /** True if guest is using in-memory storage (no path prefixing needed) */
    get usesInMemoryStorage() { return mode === 'guest' && guestBackendType === 'memory'; },

    // Start hosting a session
    startHosting(code: string, wsId: string, isReadOnly: boolean = false, selectedAudience: string | null = null) {
      mode = 'hosting';
      joinCode = code;
      workspaceId = wsId;
      connected = true;
      connecting = false;
      error = null;
      peerCount = 0;
      peers = [];
      readOnly = isReadOnly;
      audience = selectedAudience;
    },

    // Join as guest
    startGuest(code: string, wsId: string, host?: string, backendType: 'memory' | 'opfs' = 'memory', isReadOnly: boolean = false) {
      mode = 'guest';
      joinCode = code;
      workspaceId = wsId;
      hostName = host ?? null;
      guestBackendType = backendType;
      connected = true;
      connecting = false;
      error = null;
      readOnly = isReadOnly;
    },

    // Set read-only mode (host can toggle during session)
    setReadOnly(value: boolean) {
      readOnly = value;
    },

    // Set audience filter (only before session starts)
    setAudience(value: string | null) {
      audience = value;
    },

    // Set connecting state
    setConnecting(value: boolean) {
      connecting = value;
      if (value) {
        error = null;
      }
    },

    // Set connected state
    setConnected(value: boolean) {
      connected = value;
    },

    // Set error
    setError(msg: string | null) {
      error = msg;
      connecting = false;
    },

    // Update peer count (for host)
    setPeerCount(count: number) {
      peerCount = count;
    },

    // Add peer (for host)
    addPeer(peerId: string, name?: string) {
      peers = [...peers, { id: peerId, name, joinedAt: new Date() }];
      peerCount = peers.length;
    },

    // Remove peer (for host)
    removePeer(peerId: string) {
      peers = peers.filter(p => p.id !== peerId);
      peerCount = peers.length;
    },

    // Set session WebSocket
    setSessionWs(ws: WebSocket | null) {
      sessionWs = ws;
    },

    // End the session (both host and guest)
    endSession() {
      // Close WebSocket if open
      if (sessionWs && sessionWs.readyState === WebSocket.OPEN) {
        sessionWs.close();
      }

      // Reset all state
      mode = 'idle';
      joinCode = null;
      workspaceId = null;
      connected = false;
      connecting = false;
      error = null;
      peerCount = 0;
      peers = [];
      hostName = null;
      sessionWs = null;
      guestBackendType = null;
      readOnly = false;
      audience = null;
    },

    // Reset to idle (internal use)
    reset() {
      mode = 'idle';
      joinCode = null;
      workspaceId = null;
      connected = false;
      connecting = false;
      error = null;
      peerCount = 0;
      peers = [];
      hostName = null;
      sessionWs = null;
      guestBackendType = null;
      readOnly = false;
      audience = null;
    },
  };
}

// ============================================================================
// Convenience export
// ============================================================================

export const shareSessionStore = getShareSessionStore();
