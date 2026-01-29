/**
 * Multiplexed Body Sync - Single WebSocket for all body document syncs.
 *
 * Instead of creating a separate WebSocket connection for every file,
 * this uses a single connection with message framing to identify files.
 *
 * Message framing format: [varUint(pathLen)] [pathBytes (UTF-8)] [message]
 */

import type { Backend } from '../backend/interface';

/**
 * Configuration for the multiplexed body sync transport.
 */
export interface MultiplexedBodySyncOptions {
  /** WebSocket server URL (e.g., "wss://sync.example.com/sync"). */
  serverUrl: string;
  /** Workspace ID for authentication routing. */
  workspaceId: string;
  /** Backend for executing Rust commands. */
  backend: Backend;
  /** Optional auth token for authenticated sync. */
  authToken?: string;
  /** Optional session code for share session sync. */
  sessionCode?: string;
  /** Callback when connection status changes. */
  onStatusChange?: (connected: boolean) => void;
}

/**
 * Per-file subscription callbacks.
 */
interface FileSubscription {
  /** Called when a sync message is received for this file. */
  onMessage: (msg: Uint8Array) => Promise<void>;
  /** Called when initial sync completes for this file. */
  onSynced?: () => void;
}

/**
 * Multiplexed body sync transport.
 *
 * Manages a single WebSocket connection for all body document syncs,
 * using message framing to route messages to the correct file.
 */
export class MultiplexedBodySync {
  private ws: WebSocket | null = null;
  private readonly options: MultiplexedBodySyncOptions;
  private destroyed = false;
  private reconnectAttempts = 0;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private readonly maxReconnectAttempts = 10;

  /** Per-file callbacks: file_path -> callbacks */
  private fileCallbacks = new Map<string, FileSubscription>();

  /** Pending SyncStep1 sends for files subscribed while disconnected */
  private pendingSubscriptions = new Set<string>();

  constructor(options: MultiplexedBodySyncOptions) {
    this.options = options;
  }

  /**
   * Connect to the sync server.
   */
  async connect(): Promise<void> {
    if (this.destroyed || this.ws) return;

    const url = this.buildUrl();
    console.log('[MultiplexedBodySync] Connecting to', url);

    this.ws = new WebSocket(url);
    this.ws.binaryType = 'arraybuffer';

    this.ws.onopen = async () => {
      console.log('[MultiplexedBodySync] Connected');
      this.reconnectAttempts = 0;
      this.options.onStatusChange?.(true);

      // Send SyncStep1 for any files that were subscribed while disconnected
      for (const filePath of this.pendingSubscriptions) {
        await this.sendSyncStep1(filePath);
      }
      this.pendingSubscriptions.clear();
    };

    this.ws.onmessage = async (event) => {
      if (this.destroyed) return;
      const data = new Uint8Array(event.data as ArrayBuffer);

      // Unframe: read file path prefix
      const unframed = this.unframeMessage(data);
      if (!unframed.filePath) {
        console.warn('[MultiplexedBodySync] Invalid framed message');
        return;
      }

      // Route to file-specific callback
      const callbacks = this.fileCallbacks.get(unframed.filePath);
      if (callbacks) {
        await callbacks.onMessage(unframed.message);
      }
    };

    this.ws.onclose = () => {
      this.ws = null;
      this.options.onStatusChange?.(false);
      if (!this.destroyed) {
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (e) => {
      console.error('[MultiplexedBodySync] Error:', e);
    };
  }

  /**
   * Subscribe to sync for a specific file.
   * Sends initial SyncStep1 when subscribed.
   */
  async subscribe(
    filePath: string,
    onMessage: (msg: Uint8Array) => Promise<void>,
    onSynced?: () => void
  ): Promise<void> {
    this.fileCallbacks.set(filePath, { onMessage, onSynced });

    // Send initial SyncStep1 for this file
    if (this.isConnected) {
      await this.sendSyncStep1(filePath);
    } else {
      // Queue for when we connect
      this.pendingSubscriptions.add(filePath);
    }
  }

  /**
   * Unsubscribe from sync for a specific file.
   */
  unsubscribe(filePath: string): void {
    this.fileCallbacks.delete(filePath);
    this.pendingSubscriptions.delete(filePath);
  }

  /**
   * Check if a file is currently subscribed.
   */
  isSubscribed(filePath: string): boolean {
    return this.fileCallbacks.has(filePath);
  }

  /**
   * Send a sync message for a specific file.
   */
  send(filePath: string, message: Uint8Array): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      console.warn('[MultiplexedBodySync] Not connected, cannot send message');
      return;
    }

    const framed = this.frameMessage(filePath, message);
    this.ws.send(framed);
  }

  /**
   * Check if connected to the server.
   */
  get isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }

  /**
   * Get the number of active file subscriptions.
   */
  get subscriptionCount(): number {
    return this.fileCallbacks.size;
  }

  /**
   * Destroy the transport. Cannot be reconnected after this.
   */
  destroy(): void {
    this.destroyed = true;
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    if (this.ws) {
      this.ws.close(1000, 'Client destroying');
      this.ws = null;
    }
    this.fileCallbacks.clear();
    this.pendingSubscriptions.clear();
    this.options.onStatusChange?.(false);
  }

  // =========================================================================
  // Private Methods
  // =========================================================================

  /**
   * Build the WebSocket URL with multiplexed=true parameter.
   */
  private buildUrl(): string {
    let url = this.options.serverUrl;

    // Add workspace ID
    if (!url.includes('?')) {
      url += `?doc=${encodeURIComponent(this.options.workspaceId)}`;
    } else {
      url += `&doc=${encodeURIComponent(this.options.workspaceId)}`;
    }

    // Add multiplexed=true to enable multiplexed body sync mode
    url += '&multiplexed=true';

    // Add auth token if provided
    if (this.options.authToken) {
      url += `&token=${encodeURIComponent(this.options.authToken)}`;
    }

    // Add session code if provided
    if (this.options.sessionCode) {
      url += `&session=${encodeURIComponent(this.options.sessionCode)}`;
    }

    return url;
  }

  /**
   * Frame a message with file path prefix.
   * Format: [varUint(pathLen)] [pathBytes] [message]
   */
  private frameMessage(filePath: string, message: Uint8Array): Uint8Array {
    const pathBytes = new TextEncoder().encode(filePath);
    const pathLen = this.encodeVarUint(pathBytes.length);

    const result = new Uint8Array(pathLen.length + pathBytes.length + message.length);
    result.set(pathLen, 0);
    result.set(pathBytes, pathLen.length);
    result.set(message, pathLen.length + pathBytes.length);
    return result;
  }

  /**
   * Unframe a message to extract file path and payload.
   */
  private unframeMessage(data: Uint8Array): { filePath: string | null; message: Uint8Array } {
    const { value: pathLen, bytesRead } = this.decodeVarUint(data);
    if (pathLen === null || bytesRead + pathLen > data.length) {
      return { filePath: null, message: new Uint8Array(0) };
    }

    const pathBytes = data.slice(bytesRead, bytesRead + pathLen);
    const filePath = new TextDecoder().decode(pathBytes);
    const message = data.slice(bytesRead + pathLen);

    return { filePath, message };
  }

  /**
   * Encode a variable-length unsigned integer.
   * Uses 7 bits per byte with MSB as continuation flag.
   */
  private encodeVarUint(num: number): Uint8Array {
    const bytes: number[] = [];
    while (num >= 0x80) {
      bytes.push((num & 0x7F) | 0x80);
      num >>>= 7;
    }
    bytes.push(num);
    return new Uint8Array(bytes);
  }

  /**
   * Decode a variable-length unsigned integer.
   */
  private decodeVarUint(data: Uint8Array): { value: number | null; bytesRead: number } {
    let value = 0;
    let shift = 0;
    let bytesRead = 0;

    for (let i = 0; i < data.length && shift < 35; i++) {
      const byte = data[i];
      bytesRead++;
      value |= (byte & 0x7F) << shift;
      if ((byte & 0x80) === 0) {
        return { value, bytesRead };
      }
      shift += 7;
    }

    return { value: null, bytesRead: 0 };
  }

  /**
   * Send SyncStep1 for a specific file via Rust backend.
   */
  private async sendSyncStep1(filePath: string): Promise<void> {
    try {
      // Initialize body sync in Rust
      await this.options.backend.execute({
        type: 'InitBodySync' as any,
        params: { doc_name: filePath },
      } as any);

      // Get SyncStep1 message
      const response = await this.options.backend.execute({
        type: 'CreateBodySyncStep1' as any,
        params: { doc_name: filePath },
      } as any);

      if ((response.type as string) === 'Binary' && (response as any).data) {
        const bytes = new Uint8Array((response as any).data);
        this.send(filePath, bytes);
      }
    } catch (error) {
      console.error(`[MultiplexedBodySync] Failed to send SyncStep1 for ${filePath}:`, error);
    }
  }

  /**
   * Schedule a reconnection with exponential backoff.
   */
  private scheduleReconnect(): void {
    if (this.destroyed || this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log('[MultiplexedBodySync] Max reconnect attempts reached');
      return;
    }

    // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s (max)
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 32000);
    this.reconnectAttempts++;

    console.log(`[MultiplexedBodySync] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`);

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = null;
      this.connect();
    }, delay);
  }
}
