/**
 * Hocuspocus Bridge for WebSocket sync with Rust CRDT.
 *
 * Routes WebSocket messages through the Rust sync protocol
 * (diaryx_core/crdt/sync.rs) which handles Y-sync wire format.
 *
 * This replaces the direct JS Y.js <-> Hocuspocus communication
 * with Rust CRDT as the intermediary.
 */

import type { RustCrdtApi } from './rustCrdtApi';
import type { YDocProxy } from './yDocProxy';

export interface HocuspocusBridgeOptions {
  /** WebSocket URL for Hocuspocus server */
  url: string;
  /** Document name for sync (e.g., 'workspace' or file path) */
  docName: string;
  /** Rust CRDT API instance */
  rustApi: RustCrdtApi;
  /** Optional YDocProxy to sync with */
  yDocProxy?: YDocProxy;
  /** Auth token (optional) */
  token?: string;
  /** Callback when connection status changes */
  onStatusChange?: (connected: boolean) => void;
  /** Callback when sync is complete */
  onSynced?: () => void;
  /** Callback when an update is received from server */
  onUpdate?: (update: Uint8Array) => void;
}

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'syncing' | 'synced';

type EventCallback = (...args: any[]) => void;

/**
 * Bridge for Hocuspocus WebSocket communication via Rust CRDT.
 *
 * Implements HocuspocusProvider-compatible event interface for use with TipTap Editor.
 */
export class HocuspocusBridge {
  private ws: WebSocket | null = null;
  private url: string;
  private docName: string;
  private rustApi: RustCrdtApi;
  private yDocProxy?: YDocProxy;
  private token?: string;
  private onStatusChange?: (connected: boolean) => void;
  private onSyncedCallback?: () => void;
  private onUpdate?: (update: Uint8Array) => void;

  private status: ConnectionStatus = 'disconnected';
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 10;
  private destroyed = false;

  // Queue for updates when disconnected to prevent data loss
  private pendingUpdates: Uint8Array[] = [];
  private maxPendingUpdates = 100;

  // HocuspocusProvider-compatible event interface
  private eventListeners: Map<string, Set<EventCallback>> = new Map();

  /** HocuspocusProvider compatibility: true when synced with server */
  synced = false;

  constructor(options: HocuspocusBridgeOptions) {
    this.url = options.url;
    this.docName = options.docName;
    this.rustApi = options.rustApi;
    this.yDocProxy = options.yDocProxy;
    this.token = options.token;
    this.onStatusChange = options.onStatusChange;
    this.onSyncedCallback = options.onSynced;
    this.onUpdate = options.onUpdate;
  }

  // ============================================================================
  // HocuspocusProvider-compatible event interface
  // ============================================================================

  /**
   * Subscribe to an event (HocuspocusProvider compatibility).
   * Supported events: 'synced', 'status', 'disconnect'
   */
  on(event: string, callback: EventCallback): this {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, new Set());
    }
    this.eventListeners.get(event)!.add(callback);
    return this;
  }

  /**
   * Unsubscribe from an event (HocuspocusProvider compatibility).
   */
  off(event: string, callback: EventCallback): this {
    const listeners = this.eventListeners.get(event);
    if (listeners) {
      listeners.delete(callback);
    }
    return this;
  }

  /**
   * Emit an event to all listeners.
   */
  private emit(event: string, ...args: any[]): void {
    const listeners = this.eventListeners.get(event);
    if (listeners) {
      for (const callback of listeners) {
        try {
          callback(...args);
        } catch (e) {
          console.error(`[HocuspocusBridge] Event handler error for '${event}':`, e);
        }
      }
    }
  }

  /**
   * Get current connection status.
   */
  getStatus(): ConnectionStatus {
    return this.status;
  }

  /**
   * Check if connected and synced.
   */
  isSynced(): boolean {
    return this.status === 'synced';
  }

  /**
   * Connect to the Hocuspocus server.
   */
  async connect(): Promise<void> {
    if (this.destroyed) return;
    if (this.ws?.readyState === WebSocket.OPEN) return;

    this.setStatus('connecting');

    try {
      // Build WebSocket URL with room name and optional token
      const wsUrl = new URL(this.url);
      wsUrl.searchParams.set('room', this.docName);
      if (this.token) {
        wsUrl.searchParams.set('token', this.token);
      }

      this.ws = new WebSocket(wsUrl.toString());
      this.ws.binaryType = 'arraybuffer';

      this.ws.onopen = () => this.handleOpen();
      this.ws.onmessage = (event) => this.handleMessage(event);
      this.ws.onclose = (event) => this.handleClose(event);
      this.ws.onerror = (error) => this.handleError(error);
    } catch (error) {
      console.error('[HocuspocusBridge] Connection error:', error);
      this.scheduleReconnect();
    }
  }

  /**
   * Disconnect from the server.
   */
  disconnect(): void {
    this.cancelReconnect();
    if (this.ws) {
      this.ws.onclose = null; // Prevent reconnect
      this.ws.close();
      this.ws = null;
    }
    this.setStatus('disconnected');
  }

  /**
   * Destroy the bridge and cleanup resources.
   */
  destroy(): void {
    this.destroyed = true;
    this.disconnect();
  }

  /**
   * Broadcast a local update to the server.
   * If not connected, queues the update for transmission when reconnected.
   */
  async broadcastUpdate(update: Uint8Array): Promise<void> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      // Queue update for later transmission to prevent data loss
      if (this.pendingUpdates.length < this.maxPendingUpdates) {
        this.pendingUpdates.push(update);
        console.log('[HocuspocusBridge] Update queued for reconnection, queue size:', this.pendingUpdates.length);
      } else {
        console.error('[HocuspocusBridge] Pending update queue full, update dropped');
      }
      return;
    }

    try {
      // Wrap update in Y-sync message format via Rust
      const message = await this.rustApi.createUpdateMessage(update, this.docName);
      this.ws.send(message);
    } catch (error) {
      console.error('[HocuspocusBridge] Broadcast error:', error);
      // Queue the update if send failed
      if (this.pendingUpdates.length < this.maxPendingUpdates) {
        this.pendingUpdates.push(update);
      }
    }
  }

  /**
   * Flush all pending updates to the server.
   * Called after sync is complete to ensure queued updates are sent.
   */
  private async flushPendingUpdates(): Promise<void> {
    if (this.pendingUpdates.length === 0) return;
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

    console.log('[HocuspocusBridge] Flushing', this.pendingUpdates.length, 'pending updates');
    const updates = [...this.pendingUpdates];
    this.pendingUpdates = [];

    for (const update of updates) {
      try {
        const message = await this.rustApi.createUpdateMessage(update, this.docName);
        this.ws.send(message);
      } catch (error) {
        console.error('[HocuspocusBridge] Failed to flush update:', error);
        // Re-queue failed updates
        this.pendingUpdates.push(update);
      }
    }
  }

  // Private methods

  private setStatus(status: ConnectionStatus): void {
    const previousStatus = this.status;
    this.status = status;
    const connected = status === 'connected' || status === 'syncing' || status === 'synced';
    this.onStatusChange?.(connected);

    // Emit HocuspocusProvider-compatible events
    this.emit('status', { status });

    if (status === 'synced' && previousStatus !== 'synced') {
      this.synced = true;
      this.emit('synced', { state: 'connected' });
      this.onSyncedCallback?.();

      // Flush any pending updates that accumulated during disconnection
      this.flushPendingUpdates().catch((error) => {
        console.error('[HocuspocusBridge] Failed to flush pending updates:', error);
      });
    } else if (status === 'disconnected' && previousStatus !== 'disconnected') {
      this.synced = false;
      this.emit('disconnect', { event: { code: 1000 } });
    }
  }

  private async handleOpen(): Promise<void> {
    console.log('[HocuspocusBridge] Connected to', this.url);
    this.reconnectAttempts = 0;
    this.setStatus('syncing');

    try {
      // Send SyncStep1 to initiate sync
      const syncStep1 = await this.rustApi.createSyncStep1(this.docName);
      this.ws?.send(syncStep1);
    } catch (error) {
      console.error('[HocuspocusBridge] Failed to send SyncStep1:', error);
    }
  }

  private async handleMessage(event: MessageEvent): Promise<void> {
    if (this.destroyed) return;

    try {
      const message = new Uint8Array(event.data as ArrayBuffer);

      // Route message through Rust sync protocol
      const response = await this.rustApi.handleSyncMessage(message, this.docName);

      // Send response if needed
      if (response && response.length > 0) {
        this.ws?.send(response);
      }

      // Check if this was a SyncStep2 (we're now synced)
      // Message type is in first byte: 0=Sync, then subtype: 0=Step1, 1=Step2, 2=Update
      if (message[0] === 0 && message[1] === 1) {
        // SyncStep2 received - we're synced
        this.setStatus('synced');
      }

      // If it's an update message, sync to YDocProxy and notify
      if (message[0] === 0 && message[1] === 2) {
        // Extract update from message (skip 2 byte header)
        const update = message.slice(2);

        // Apply to YDocProxy if present
        if (this.yDocProxy) {
          await this.yDocProxy.applyRemoteUpdate(update);
        }

        this.onUpdate?.(update);
      }
    } catch (error) {
      console.error('[HocuspocusBridge] Message handling error:', error);
    }
  }

  private handleClose(event: CloseEvent): void {
    console.log('[HocuspocusBridge] Disconnected:', event.code, event.reason);
    this.ws = null;
    this.setStatus('disconnected');

    if (!this.destroyed) {
      this.scheduleReconnect();
    }
  }

  private handleError(error: Event): void {
    console.error('[HocuspocusBridge] WebSocket error:', error);
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return;
    if (this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.error('[HocuspocusBridge] Max reconnect attempts reached');
      return;
    }

    this.cancelReconnect();

    // Exponential backoff with jitter
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
    const jitter = Math.random() * 1000;

    console.log(`[HocuspocusBridge] Reconnecting in ${delay + jitter}ms (attempt ${this.reconnectAttempts + 1})`);

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectAttempts++;
      this.connect();
    }, delay + jitter);
  }

  private cancelReconnect(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
  }
}

/**
 * Create a Hocuspocus bridge for a document.
 */
export function createHocuspocusBridge(options: HocuspocusBridgeOptions): HocuspocusBridge {
  return new HocuspocusBridge(options);
}
