/**
 * Multiplexed Body Sync - Single WebSocket for all body document syncs.
 *
 * Instead of creating a separate WebSocket connection for every file,
 * this uses a single connection with message framing to identify files.
 *
 * Message framing format: [varUint(pathLen)] [pathBytes (UTF-8)] [message]
 */

import type { Backend } from "../backend/interface";

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
  /** Callback for sync progress updates from server. */
  onProgress?: (completed: number, total: number) => void;
  /** Callback when sync_complete is received from server. */
  onSyncComplete?: (filesSynced: number) => void;
  /** Callback for unsubscribed file messages (allows applying updates for files not actively open). */
  onUnsubscribedMessage?: (
    filePath: string,
    message: Uint8Array,
  ) => Promise<void>;
  /** Callback when focus list changes (files that any client is focused on). */
  onFocusListChanged?: (files: string[]) => void;
}

/**
 * Per-file subscription callbacks.
 */
interface FileSubscription {
  /** Called when a sync message is received for this file. */
  onMessage: (msg: Uint8Array) => Promise<void>;
  /** Called when initial sync completes for this file. */
  onSynced?: () => void;
  /** Promise that resolves when this file's sync is complete. */
  syncedPromise?: Promise<void>;
  /** Resolver for the synced promise. */
  syncedResolver?: () => void;
  /** Whether this file has received actual sync data (SyncStep2). */
  receivedData: boolean;
  /** Whether this file has been marked synced (via sync_complete or data received). */
  synced: boolean;
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

  /** Pending messages to send when connection is established */
  private pendingMessages = new Map<string, Uint8Array[]>();

  /** Files this client is currently focused on */
  private focusedFiles = new Set<string>();

  constructor(options: MultiplexedBodySyncOptions) {
    this.options = options;
  }

  /**
   * Connect to the sync server.
   */
  async connect(): Promise<void> {
    if (this.destroyed || this.ws) return;

    const url = this.buildUrl();
    console.log("[MultiplexedBodySync] Connecting to", url);

    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";

    this.ws.onopen = async () => {
      console.log("[MultiplexedBodySync] Connected");
      this.reconnectAttempts = 0;
      this.options.onStatusChange?.(true);

      // Send SyncStep1 for any files that were subscribed while disconnected
      for (const filePath of this.pendingSubscriptions) {
        await this.sendSyncStep1(filePath);
      }
      this.pendingSubscriptions.clear();

      // Flush any queued messages that were waiting for connection
      if (this.pendingMessages.size > 0) {
        console.log(
          `[MultiplexedBodySync] Flushing ${this.pendingMessages.size} queued file(s)`,
        );
        for (const [filePath, messages] of this.pendingMessages) {
          for (const msg of messages) {
            const framed = this.frameMessage(filePath, msg);
            this.ws!.send(framed);
            console.log(
              `[MultiplexedBodySync] Sent queued message for ${filePath}, ${msg.length} bytes`,
            );
          }
        }
        this.pendingMessages.clear();
      }

      // Resend focus list after reconnect (server clears focus on disconnect)
      if (this.focusedFiles.size > 0) {
        this.sendFocusMessage(Array.from(this.focusedFiles));
      }
    };

    this.ws.onmessage = async (event) => {
      if (this.destroyed) return;

      // Handle text messages (JSON control messages) separately from binary
      if (typeof event.data === "string") {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === "sync_progress") {
            console.log(
              `[MultiplexedBodySync] Sync progress: ${msg.completed}/${msg.total}`,
            );
            this.options.onProgress?.(msg.completed, msg.total);
          } else if (msg.type === "sync_complete") {
            console.log(
              `[MultiplexedBodySync] Sync complete: ${msg.files_synced} files`,
            );
            this.options.onSyncComplete?.(msg.files_synced);
            // Mark all subscribed files as synced and notify
            // Note: Files that haven't received data will still be marked synced
            // (they may have no remote changes, which is valid)
            for (const [filePath, callbacks] of this.fileCallbacks) {
              if (!callbacks.synced) {
                callbacks.synced = true;
                console.log(
                  `[MultiplexedBodySync] Marking ${filePath} synced (receivedData: ${callbacks.receivedData})`,
                );
                callbacks.onSynced?.();
                callbacks.syncedResolver?.();
              }
            }
          } else if (msg.type === "focus_list_changed") {
            console.log(
              `[MultiplexedBodySync] Focus list changed: ${msg.files?.length ?? 0} files`,
            );
            this.options.onFocusListChanged?.(msg.files ?? []);
          }
        } catch (e) {
          console.warn(
            "[MultiplexedBodySync] Failed to parse control message:",
            e,
          );
        }
        return;
      }

      const data = new Uint8Array(event.data as ArrayBuffer);

      // Unframe: read file path prefix
      const unframed = this.unframeMessage(data);
      if (!unframed.filePath) {
        console.warn("[MultiplexedBodySync] Invalid framed message");
        return;
      }

      // Route to file-specific callback
      const callbacks = this.fileCallbacks.get(unframed.filePath);
      if (callbacks) {
        // Mark that this file has received actual sync data
        callbacks.receivedData = true;
        await callbacks.onMessage(unframed.message);
      } else if (this.options.onUnsubscribedMessage) {
        // Handle messages for files we're not actively subscribed to
        // This ensures updates from other clients are applied even if the file isn't open
        console.log(
          `[MultiplexedBodySync] Received message for unsubscribed file: ${unframed.filePath}, forwarding to handler`,
        );
        try {
          await this.options.onUnsubscribedMessage(
            unframed.filePath,
            unframed.message,
          );
        } catch (err) {
          console.warn(
            `[MultiplexedBodySync] Failed to handle unsubscribed message for ${unframed.filePath}:`,
            err,
          );
        }
      } else {
        console.log(
          `[MultiplexedBodySync] Dropped message for unsubscribed file: ${unframed.filePath} (no handler)`,
        );
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
      console.error("[MultiplexedBodySync] Error:", e);
    };
  }

  /**
   * Subscribe to sync for a specific file.
   * Sends initial SyncStep1 when subscribed.
   * Returns immediately - use waitForSync() to wait for completion.
   */
  async subscribe(
    filePath: string,
    onMessage: (msg: Uint8Array) => Promise<void>,
    onSynced?: () => void,
  ): Promise<void> {
    // Create a promise that resolves when this file's sync completes
    let syncedResolver: () => void;
    const syncedPromise = new Promise<void>((resolve) => {
      syncedResolver = resolve;
    });

    this.fileCallbacks.set(filePath, {
      onMessage,
      onSynced,
      syncedPromise,
      syncedResolver: syncedResolver!,
      receivedData: false,
      synced: false,
    });

    // Send initial SyncStep1 for this file
    if (this.isConnected) {
      await this.sendSyncStep1(filePath);
    } else {
      // Queue for when we connect
      this.pendingSubscriptions.add(filePath);
    }

    // Returns immediately - caller can use waitForSync() to wait for completion
  }

  /**
   * Wait for a specific file's sync to complete.
   * Returns immediately if already synced or not subscribed.
   */
  async waitForSync(filePath: string, timeoutMs = 30000): Promise<boolean> {
    const callbacks = this.fileCallbacks.get(filePath);
    if (!callbacks?.syncedPromise) {
      return true; // Not subscribed or already synced
    }

    try {
      await Promise.race([
        callbacks.syncedPromise,
        new Promise<void>((_, reject) =>
          setTimeout(() => reject(new Error("Sync timeout")), timeoutMs),
        ),
      ]);
      return true;
    } catch {
      return false;
    }
  }

  /**
   * Wait for all subscribed files' sync to complete.
   */
  async waitForAllSyncs(timeoutMs = 60000): Promise<boolean> {
    const promises = Array.from(this.fileCallbacks.values())
      .filter((cb) => cb.syncedPromise)
      .map((cb) => cb.syncedPromise!);

    if (promises.length === 0) {
      return true;
    }

    try {
      await Promise.race([
        Promise.all(promises),
        new Promise<void>((_, reject) =>
          setTimeout(() => reject(new Error("Sync timeout")), timeoutMs),
        ),
      ]);
      return true;
    } catch {
      return false;
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
   * If not connected, queues the message to be sent when connection is established.
   */
  send(filePath: string, message: Uint8Array): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      // Queue message for when connected
      if (!this.pendingMessages.has(filePath)) {
        this.pendingMessages.set(filePath, []);
      }
      this.pendingMessages.get(filePath)!.push(message);
      console.log(
        `[MultiplexedBodySync] Queued message for ${filePath} (not connected), ${message.length} bytes`,
      );
      return;
    }

    const framed = this.frameMessage(filePath, message);
    this.ws.send(framed);
    console.log(
      `[MultiplexedBodySync] Sent message for ${filePath}, ${message.length} bytes`,
    );
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
   * Get sync status for a specific file.
   * Returns null if not subscribed.
   */
  getFileSyncStatus(
    filePath: string,
  ): { receivedData: boolean; synced: boolean } | null {
    const callbacks = this.fileCallbacks.get(filePath);
    if (!callbacks) return null;
    return {
      receivedData: callbacks.receivedData,
      synced: callbacks.synced,
    };
  }

  /**
   * Get counts of files by sync status.
   */
  getSyncStatusCounts(): {
    total: number;
    synced: number;
    receivedData: number;
  } {
    let synced = 0;
    let receivedData = 0;
    for (const callbacks of this.fileCallbacks.values()) {
      if (callbacks.synced) synced++;
      if (callbacks.receivedData) receivedData++;
    }
    return {
      total: this.fileCallbacks.size,
      synced,
      receivedData,
    };
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
      this.ws.close(1000, "Client destroying");
      this.ws = null;
    }
    this.fileCallbacks.clear();
    this.pendingSubscriptions.clear();
    this.pendingMessages.clear();
    this.options.onStatusChange?.(false);
  }

  // =========================================================================
  // Focus API - Focus-based sync subscription
  // =========================================================================

  /**
   * Focus on specific files for sync.
   *
   * Sends a focus message to the server indicating which files the client
   * is currently interested in syncing. Other clients will receive a
   * `focus_list_changed` notification and can subscribe to sync updates
   * for these files.
   *
   * Call this when a file is opened in the editor.
   *
   * @param filePaths - Array of file paths to focus on
   */
  focus(filePaths: string[]): void {
    // Track focused files locally so we can resend on reconnect
    for (const filePath of filePaths) {
      this.focusedFiles.add(filePath);
    }

    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      console.warn(
        "[MultiplexedBodySync] Cannot send focus message: not connected",
      );
      return;
    }

    this.sendFocusMessage(filePaths);
  }

  /**
   * Unfocus specific files.
   *
   * Sends an unfocus message to the server indicating the client is no
   * longer interested in syncing these files.
   *
   * Call this when a file is closed in the editor.
   *
   * @param filePaths - Array of file paths to unfocus
   */
  unfocus(filePaths: string[]): void {
    // Update local focus state
    for (const filePath of filePaths) {
      this.focusedFiles.delete(filePath);
    }

    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      console.warn(
        "[MultiplexedBodySync] Cannot send unfocus message: not connected",
      );
      return;
    }

    const unfocusMsg = JSON.stringify({
      type: "unfocus",
      files: filePaths,
    });
    this.ws.send(unfocusMsg);
    console.log(
      `[MultiplexedBodySync] Sent unfocus for ${filePaths.length} files`,
    );
  }

  // =========================================================================
  // v2 Wire Format (siphonophore)
  // These methods are available for v2 protocol migration but not yet used.
  // =========================================================================

  /**
   * Frame a message for v2 protocol with fixed u8 length prefix.
   *
   * Format: `[u8: doc_id_len] [doc_id_bytes] [payload]`
   *
   * @param docId - The document ID (e.g., "body:ws123/journal/2024.md")
   * @param message - The sync message payload
   * @returns Framed message ready to send over WebSocket
   * @internal Reserved for v2 protocol migration
   */
  protected frameMessageV2(docId: string, message: Uint8Array): Uint8Array {
    const docIdBytes = new TextEncoder().encode(docId);
    const len = Math.min(docIdBytes.length, 255);
    const result = new Uint8Array(1 + len + message.length);
    result[0] = len;
    result.set(docIdBytes.subarray(0, len), 1);
    result.set(message, 1 + len);
    return result;
  }

  /**
   * Unframe a v2 message with fixed u8 length prefix.
   *
   * @param data - The framed message data
   * @returns Object with docId (null if invalid) and message payload
   * @internal Reserved for v2 protocol migration
   */
  protected unframeMessageV2(data: Uint8Array): {
    docId: string | null;
    message: Uint8Array;
  } {
    if (data.length < 1) {
      return { docId: null, message: new Uint8Array(0) };
    }
    const len = data[0];
    if (data.length < 1 + len) {
      return { docId: null, message: new Uint8Array(0) };
    }
    const docId = new TextDecoder().decode(data.slice(1, 1 + len));
    return { docId, message: data.slice(1 + len) };
  }

  /**
   * Format a body document ID for the v2 protocol.
   *
   * @param filePath - The file path within the workspace
   * @returns The formatted doc ID (e.g., "body:ws123/journal/2024.md")
   * @internal Reserved for v2 protocol migration
   */
  protected formatBodyDocIdV2(filePath: string): string {
    return `body:${this.options.workspaceId}/${filePath}`;
  }

  /**
   * Parse a v2 document ID to extract file path.
   *
   * @param docId - The document ID to parse
   * @returns The file path if it's a body doc for this workspace, null otherwise
   * @internal Reserved for v2 protocol migration
   */
  protected parseBodyDocIdV2(docId: string): string | null {
    const prefix = `body:${this.options.workspaceId}/`;
    if (docId.startsWith(prefix)) {
      return docId.slice(prefix.length);
    }
    return null;
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
    if (!url.includes("?")) {
      url += `?doc=${encodeURIComponent(this.options.workspaceId)}`;
    } else {
      url += `&doc=${encodeURIComponent(this.options.workspaceId)}`;
    }

    // Add multiplexed=true to enable multiplexed body sync mode
    url += "&multiplexed=true";

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

    const result = new Uint8Array(
      pathLen.length + pathBytes.length + message.length,
    );
    result.set(pathLen, 0);
    result.set(pathBytes, pathLen.length);
    result.set(message, pathLen.length + pathBytes.length);
    return result;
  }

  /**
   * Unframe a message to extract file path and payload.
   */
  private unframeMessage(data: Uint8Array): {
    filePath: string | null;
    message: Uint8Array;
  } {
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
      bytes.push((num & 0x7f) | 0x80);
      num >>>= 7;
    }
    bytes.push(num);
    return new Uint8Array(bytes);
  }

  /**
   * Decode a variable-length unsigned integer.
   */
  private decodeVarUint(data: Uint8Array): {
    value: number | null;
    bytesRead: number;
  } {
    let value = 0;
    let shift = 0;
    let bytesRead = 0;

    for (let i = 0; i < data.length && shift < 35; i++) {
      const byte = data[i];
      bytesRead++;
      value |= (byte & 0x7f) << shift;
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
        type: "InitBodySync" as any,
        params: { doc_name: filePath },
      } as any);

      // Get SyncStep1 message
      const response = await this.options.backend.execute({
        type: "CreateBodySyncStep1" as any,
        params: { doc_name: filePath },
      } as any);

      if ((response.type as string) === "Binary" && (response as any).data) {
        const bytes = new Uint8Array((response as any).data);
        this.send(filePath, bytes);
      }
    } catch (error) {
      console.error(
        `[MultiplexedBodySync] Failed to send SyncStep1 for ${filePath}:`,
        error,
      );
    }
  }

  /**
   * Schedule a reconnection with exponential backoff.
   */
  private scheduleReconnect(): void {
    if (this.destroyed || this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log("[MultiplexedBodySync] Max reconnect attempts reached");
      return;
    }

    // Exponential backoff: 1s, 2s, 4s, 8s, 16s, 32s (max)
    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 32000);
    this.reconnectAttempts++;

    console.log(
      `[MultiplexedBodySync] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`,
    );

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = null;
      this.connect();
    }, delay);
  }

  private sendFocusMessage(filePaths: string[]): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      return;
    }

    const focusMsg = JSON.stringify({
      type: "focus",
      files: filePaths,
    });
    this.ws.send(focusMsg);
    console.log(
      `[MultiplexedBodySync] Sent focus for ${filePaths.length} files`,
    );
  }
}
