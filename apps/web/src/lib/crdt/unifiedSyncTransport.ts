/**
 * Unified Sync Transport for v2 protocol (siphonophore).
 *
 * This transport uses a single WebSocket connection to the /sync2 endpoint,
 * handling both workspace and body document synchronization via doc_id prefixes.
 *
 * Wire format (v2):
 * - Binary messages: `[u8: doc_id_len] [doc_id_bytes] [y-sync payload]`
 * - Text messages: JSON control messages (unchanged from v1)
 *
 * Doc ID format:
 * - Workspace: `workspace:{workspace_id}`
 * - Body: `body:{workspace_id}/{file_path}`
 */

import type { Backend } from "../backend/interface";

/**
 * Configuration for the unified sync transport.
 */
export interface UnifiedSyncTransportOptions {
  /** WebSocket server URL (will be modified to use /sync2). */
  serverUrl: string;
  /** Workspace ID for document namespacing. */
  workspaceId: string;
  /** Backend for executing Rust commands. */
  backend: Backend;
  /** Whether to write changes to disk. */
  writeToDisk: boolean;
  /** Optional auth token for authenticated sync. */
  authToken?: string;
  /** Optional session code for share session sync. */
  sessionCode?: string;
  /** Callback when connection status changes. */
  onStatusChange?: (connected: boolean) => void;
  /** Callback when workspace sync completes. */
  onWorkspaceSynced?: () => void;
  /** Callback when workspace files change. */
  onFilesChanged?: (changedFiles: string[]) => void;
  /** Callback for sync progress updates. */
  onProgress?: (completed: number, total: number) => void;
  /** Callback when sync_complete is received from server. */
  onSyncComplete?: (filesSynced: number) => void;
  /** Callback when focus list changes (files that any client is focused on). */
  onFocusListChanged?: (files: string[]) => void;
}

/**
 * Per-body-file subscription callbacks.
 */
interface BodySubscription {
  /** Called when a sync message is received for this file. */
  onMessage: (msg: Uint8Array) => Promise<void>;
  /** Called when initial sync completes for this file. */
  onSynced?: () => void;
  /** Promise that resolves when this file's sync is complete. */
  syncedPromise?: Promise<void>;
  /** Resolver for the synced promise. */
  syncedResolver?: () => void;
  /** Whether this file has received actual sync data. */
  receivedData: boolean;
  /** Whether this file has been marked synced. */
  synced: boolean;
}

/**
 * Unified sync transport for v2 protocol.
 *
 * Manages a single WebSocket connection for both workspace and body document syncs,
 * using v2 message framing (u8 length prefix) to route messages.
 */
export class UnifiedSyncTransport {
  private ws: WebSocket | null = null;
  private readonly options: UnifiedSyncTransportOptions;
  private destroyed = false;
  private reconnectAttempts = 0;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private readonly maxReconnectAttempts = 10;

  /** Whether initial workspace sync is complete. */
  private workspaceSynced = false;

  /** Per-body-file callbacks: file_path -> callbacks */
  private bodyCallbacks = new Map<string, BodySubscription>();

  /** Pending body subscriptions for when we reconnect */
  private pendingBodySubscriptions = new Set<string>();

  /** Pending messages to send when connection is established */
  private pendingMessages: Uint8Array[] = [];

  /** Files this client is currently focused on */
  private focusedFiles = new Set<string>();

  constructor(options: UnifiedSyncTransportOptions) {
    this.options = options;
  }

  // =========================================================================
  // Public API
  // =========================================================================

  /**
   * Connect to the sync server.
   */
  async connect(): Promise<void> {
    if (this.destroyed || this.ws) return;

    const url = this.buildUrl();
    console.log("[UnifiedSyncTransport] Connecting to", url);

    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";

    this.ws.onopen = async () => {
      console.log("[UnifiedSyncTransport] Connected");
      this.reconnectAttempts = 0;
      this.options.onStatusChange?.(true);

      // Send workspace SyncStep1 first
      await this.sendWorkspaceSyncStep1();

      // Send SyncStep1 for any body files that were subscribed while disconnected
      for (const filePath of this.pendingBodySubscriptions) {
        await this.sendBodySyncStep1(filePath);
      }
      this.pendingBodySubscriptions.clear();

      // Flush any queued messages
      if (this.pendingMessages.length > 0) {
        console.log(
          `[UnifiedSyncTransport] Flushing ${this.pendingMessages.length} queued messages`,
        );
        for (const msg of this.pendingMessages) {
          this.ws!.send(msg);
        }
        this.pendingMessages = [];
      }

      // Resend focus list after reconnect
      if (this.focusedFiles.size > 0) {
        this.sendFocusMessage(Array.from(this.focusedFiles));
      }
    };

    this.ws.onmessage = async (event) => {
      if (this.destroyed) return;

      // Handle text messages (JSON control messages)
      if (typeof event.data === "string") {
        await this.handleControlMessage(event.data);
        return;
      }

      // Handle binary messages (sync protocol)
      const data = new Uint8Array(event.data as ArrayBuffer);
      await this.handleBinaryMessage(data);
    };

    this.ws.onclose = () => {
      this.ws = null;
      this.options.onStatusChange?.(false);
      if (!this.destroyed) {
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (e) => {
      console.error("[UnifiedSyncTransport] Error:", e);
    };
  }

  /**
   * Disconnect from the sync server.
   */
  disconnect(): void {
    if (this.reconnectTimeout) {
      clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }

    if (this.ws) {
      this.ws.close(1000, "Client disconnecting");
      this.ws = null;
    }

    this.options.onStatusChange?.(false);
  }

  /**
   * Destroy the transport. Cannot be reconnected after this.
   */
  destroy(): void {
    this.destroyed = true;
    this.disconnect();
    this.bodyCallbacks.clear();
    this.pendingBodySubscriptions.clear();
    this.pendingMessages = [];
  }

  /**
   * Check if connected to the server.
   */
  get isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }

  /**
   * Check if workspace is synced.
   */
  get isWorkspaceSynced(): boolean {
    return this.workspaceSynced;
  }

  /**
   * Subscribe to body sync for a specific file.
   */
  async subscribeBody(
    filePath: string,
    onMessage: (msg: Uint8Array) => Promise<void>,
    onSynced?: () => void,
  ): Promise<void> {
    let syncedResolver: () => void;
    const syncedPromise = new Promise<void>((resolve) => {
      syncedResolver = resolve;
    });

    this.bodyCallbacks.set(filePath, {
      onMessage,
      onSynced,
      syncedPromise,
      syncedResolver: syncedResolver!,
      receivedData: false,
      synced: false,
    });

    if (this.isConnected) {
      await this.sendBodySyncStep1(filePath);
    } else {
      this.pendingBodySubscriptions.add(filePath);
    }
  }

  /**
   * Unsubscribe from body sync for a specific file.
   */
  unsubscribeBody(filePath: string): void {
    this.bodyCallbacks.delete(filePath);
    this.pendingBodySubscriptions.delete(filePath);
  }

  /**
   * Wait for a specific file's body sync to complete.
   */
  async waitForBodySync(filePath: string, timeoutMs = 30000): Promise<boolean> {
    const callbacks = this.bodyCallbacks.get(filePath);
    if (!callbacks?.syncedPromise) {
      return true;
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
   * Send a body sync message for a specific file.
   */
  sendBodyMessage(filePath: string, message: Uint8Array): void {
    const docId = this.formatBodyDocId(filePath);
    const framed = this.frameMessageV2(docId, message);

    if (!this.isConnected) {
      this.pendingMessages.push(framed);
      console.log(
        `[UnifiedSyncTransport] Queued body message for ${filePath}`,
      );
      return;
    }

    this.ws!.send(framed);
  }

  /**
   * Focus on specific files for sync.
   */
  focus(filePaths: string[]): void {
    for (const filePath of filePaths) {
      this.focusedFiles.add(filePath);
    }

    if (this.isConnected) {
      this.sendFocusMessage(filePaths);
    }
  }

  /**
   * Unfocus specific files.
   */
  unfocus(filePaths: string[]): void {
    for (const filePath of filePaths) {
      this.focusedFiles.delete(filePath);
    }

    if (this.isConnected) {
      const unfocusMsg = JSON.stringify({
        type: "unfocus",
        files: filePaths,
      });
      this.ws!.send(unfocusMsg);
    }
  }

  // =========================================================================
  // v2 Wire Format
  // =========================================================================

  /**
   * Frame a message for v2 protocol with fixed u8 length prefix.
   */
  private frameMessageV2(docId: string, message: Uint8Array): Uint8Array {
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
   */
  private unframeMessageV2(data: Uint8Array): {
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
   * Format workspace document ID for v2 protocol.
   */
  private formatWorkspaceDocId(): string {
    return `workspace:${this.options.workspaceId}`;
  }

  /**
   * Format body document ID for v2 protocol.
   */
  private formatBodyDocId(filePath: string): string {
    return `body:${this.options.workspaceId}/${filePath}`;
  }

  /**
   * Parse a document ID to determine its type and extract components.
   */
  private parseDocId(
    docId: string,
  ):
    | { type: "workspace"; workspaceId: string }
    | { type: "body"; workspaceId: string; filePath: string }
    | null {
    if (docId.startsWith("workspace:")) {
      return {
        type: "workspace",
        workspaceId: docId.slice("workspace:".length),
      };
    }
    if (docId.startsWith("body:")) {
      const rest = docId.slice("body:".length);
      const slashIndex = rest.indexOf("/");
      if (slashIndex === -1) return null;
      return {
        type: "body",
        workspaceId: rest.slice(0, slashIndex),
        filePath: rest.slice(slashIndex + 1),
      };
    }
    return null;
  }

  // =========================================================================
  // Message Handlers
  // =========================================================================

  /**
   * Handle JSON control messages from server.
   */
  private async handleControlMessage(text: string): Promise<void> {
    try {
      const msg = JSON.parse(text);

      switch (msg.type) {
        case "sync_progress":
          console.log(
            `[UnifiedSyncTransport] Sync progress: ${msg.completed}/${msg.total}`,
          );
          this.options.onProgress?.(msg.completed, msg.total);
          break;

        case "sync_complete":
          console.log(
            `[UnifiedSyncTransport] Sync complete: ${msg.files_synced} files`,
          );
          this.options.onSyncComplete?.(msg.files_synced);
          // Mark all body subscriptions as synced
          for (const [filePath, callbacks] of this.bodyCallbacks) {
            if (!callbacks.synced) {
              callbacks.synced = true;
              console.log(
                `[UnifiedSyncTransport] Marking ${filePath} synced`,
              );
              callbacks.onSynced?.();
              callbacks.syncedResolver?.();
            }
          }
          break;

        case "focus_list_changed":
          console.log(
            `[UnifiedSyncTransport] Focus list changed: ${msg.files?.length ?? 0} files`,
          );
          this.options.onFocusListChanged?.(msg.files ?? []);
          break;

        case "FileManifest":
        case "CrdtState":
          // v2 protocol may still use these for handshaking
          console.log(`[UnifiedSyncTransport] Received ${msg.type}`);
          break;

        default:
          // Unknown control message, ignore
          break;
      }
    } catch (e) {
      console.warn(
        "[UnifiedSyncTransport] Failed to parse control message:",
        e,
      );
    }
  }

  /**
   * Handle binary sync messages from server.
   */
  private async handleBinaryMessage(data: Uint8Array): Promise<void> {
    const { docId, message } = this.unframeMessageV2(data);
    if (!docId) {
      console.warn("[UnifiedSyncTransport] Invalid framed message");
      return;
    }

    const parsed = this.parseDocId(docId);
    if (!parsed) {
      console.warn("[UnifiedSyncTransport] Unknown doc_id format:", docId);
      return;
    }

    if (parsed.type === "workspace") {
      await this.handleWorkspaceMessage(message);
    } else {
      await this.handleBodyMessage(parsed.filePath, message);
    }
  }

  /**
   * Handle workspace sync message.
   */
  private async handleWorkspaceMessage(message: Uint8Array): Promise<void> {
    try {
      const response = await this.options.backend.execute({
        type: "HandleWorkspaceSyncMessage" as any,
        params: {
          message: Array.from(message),
          write_to_disk: this.options.writeToDisk,
        },
      } as any);

      if ((response.type as string) === "WorkspaceSyncResult") {
        const result = response as any;

        // Send response if Rust returns one
        if (result.data?.response && result.data.response.length > 0) {
          const docId = this.formatWorkspaceDocId();
          const framed = this.frameMessageV2(
            docId,
            new Uint8Array(result.data.response),
          );
          this.ws?.send(framed);
        }

        // Handle changed files notification
        if (result.data?.changed_files && result.data.changed_files.length > 0) {
          this.options.onFilesChanged?.(result.data.changed_files);
        }

        // Mark workspace synced on first successful message
        if (!this.workspaceSynced) {
          this.workspaceSynced = true;
          this.options.onWorkspaceSynced?.();
        }
      }
    } catch (error) {
      console.error(
        "[UnifiedSyncTransport] Error handling workspace message:",
        error,
      );
    }
  }

  /**
   * Handle body sync message.
   */
  private async handleBodyMessage(
    filePath: string,
    message: Uint8Array,
  ): Promise<void> {
    const callbacks = this.bodyCallbacks.get(filePath);
    if (callbacks) {
      callbacks.receivedData = true;
      await callbacks.onMessage(message);
    } else {
      console.log(
        `[UnifiedSyncTransport] Dropped message for unsubscribed file: ${filePath}`,
      );
    }
  }

  // =========================================================================
  // Send Methods
  // =========================================================================

  /**
   * Send workspace SyncStep1 to initiate sync.
   */
  private async sendWorkspaceSyncStep1(): Promise<void> {
    try {
      const response = await this.options.backend.execute({
        type: "CreateWorkspaceSyncStep1",
      } as any);

      if ((response.type as string) === "Binary" && (response as any).data) {
        const docId = this.formatWorkspaceDocId();
        const framed = this.frameMessageV2(
          docId,
          new Uint8Array((response as any).data),
        );
        this.ws?.send(framed);
        console.log("[UnifiedSyncTransport] Sent workspace SyncStep1");
      }
    } catch (error) {
      console.error(
        "[UnifiedSyncTransport] Failed to send workspace SyncStep1:",
        error,
      );
    }
  }

  /**
   * Send body SyncStep1 for a specific file.
   */
  private async sendBodySyncStep1(filePath: string): Promise<void> {
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
        const docId = this.formatBodyDocId(filePath);
        const framed = this.frameMessageV2(
          docId,
          new Uint8Array((response as any).data),
        );
        this.ws?.send(framed);
        console.log(
          `[UnifiedSyncTransport] Sent body SyncStep1 for ${filePath}`,
        );
      }
    } catch (error) {
      console.error(
        `[UnifiedSyncTransport] Failed to send body SyncStep1 for ${filePath}:`,
        error,
      );
    }
  }

  /**
   * Send focus message to server.
   */
  private sendFocusMessage(filePaths: string[]): void {
    if (!this.isConnected) return;

    const focusMsg = JSON.stringify({
      type: "focus",
      files: filePaths,
    });
    this.ws!.send(focusMsg);
    console.log(
      `[UnifiedSyncTransport] Sent focus for ${filePaths.length} files`,
    );
  }

  // =========================================================================
  // URL and Reconnection
  // =========================================================================

  /**
   * Build the WebSocket URL for /sync2 endpoint.
   */
  private buildUrl(): string {
    // Replace /sync with /sync2, or append /sync2 if not present
    let url = this.options.serverUrl.replace(/\/sync$/, "/sync2");
    if (!url.endsWith("/sync2")) {
      url = url.replace(/\/$/, "") + "/sync2";
    }

    // Add auth token as query param
    const params = new URLSearchParams();
    if (this.options.authToken) {
      params.set("token", this.options.authToken);
    }
    if (this.options.sessionCode) {
      params.set("session", this.options.sessionCode);
    }

    const queryString = params.toString();
    return queryString ? `${url}?${queryString}` : url;
  }

  /**
   * Schedule a reconnection with exponential backoff.
   */
  private scheduleReconnect(): void {
    if (this.destroyed || this.reconnectAttempts >= this.maxReconnectAttempts) {
      console.log("[UnifiedSyncTransport] Max reconnect attempts reached");
      return;
    }

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 32000);
    this.reconnectAttempts++;

    console.log(
      `[UnifiedSyncTransport] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`,
    );

    this.reconnectTimeout = setTimeout(() => {
      this.reconnectTimeout = null;
      this.connect();
    }, delay);
  }
}

/**
 * Create a unified sync transport for v2 protocol.
 */
export function createUnifiedSyncTransport(
  options: UnifiedSyncTransportOptions,
): UnifiedSyncTransport {
  return new UnifiedSyncTransport(options);
}
