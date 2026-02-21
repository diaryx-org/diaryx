/**
 * Unified Sync Transport for v2 protocol (siphonophore).
 *
 * This transport manages a WebSocket connection to /sync2 and delegates all
 * protocol logic (handshake, message routing, framing) to WasmSyncClient
 * running in the WASM worker. The transport only handles:
 *
 * - WebSocket lifecycle (connect, disconnect, reconnect)
 * - Forwarding raw messages to/from the worker
 * - Draining outgoing messages and events after each injection
 * - Snapshot download (HTTP, main thread)
 * - Reconnection with exponential backoff
 *
 * Wire format (v2, handled by Rust):
 * - Binary messages: `[u8: doc_id_len] [doc_id_bytes] [y-sync payload]`
 * - Text messages: JSON control messages
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
  /** Backend for executing Rust commands and sync client operations. */
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
  /** Callback when session_joined is received (for share session guests). */
  onSessionJoined?: (data: { joinCode: string; workspaceId: string; readOnly: boolean }) => void;
  /** Callback when a peer joins a document. */
  onPeerJoined?: (guestId: string, peerCount: number) => void;
  /** Callback when a peer leaves a document. */
  onPeerLeft?: (guestId: string, peerCount: number) => void;
  /** Callback when the session has ended (host disconnected). */
  onSessionEnded?: () => void;
  /** Callback when a body file's content changes remotely. */
  onBodyChanged?: (filePath: string) => void;
  /** Callback when a fatal connection error occurs (e.g. server rejected protocol). */
  onError?: (message: string) => void;
}

/**
 * Unified sync transport for v2 protocol.
 *
 * Manages a WebSocket connection and delegates all protocol logic to
 * WasmSyncClient (Rust) running in the WASM worker.
 */
export class UnifiedSyncTransport {
  private ws: WebSocket | null = null;
  private readonly options: UnifiedSyncTransportOptions;
  private destroyed = false;
  private reconnectAttempts = 0;
  private reconnectTimeout: ReturnType<typeof setTimeout> | null = null;
  private readonly maxReconnectAttempts = 10;

  /** Incremented on each connect() call to detect stale async handlers. */
  private connectionGeneration = 0;

  /** Whether the sync client has been created in the worker. */
  private syncClientCreated = false;

  /** Whether initial workspace sync is complete. */
  private workspaceSynced = false;

  /** Whether handshake is complete. */
  private handshakeComplete = false;

  /** Files this client is currently focused on */
  private focusedFiles = new Set<string>();

  // -- Message batching state --
  /** Buffered binary messages waiting to be flushed. */
  private pendingBinaryMessages: Uint8Array[] = [];
  /** Buffered text messages waiting to be flushed. */
  private pendingTextMessages: string[] = [];
  /** Whether a flush is already scheduled for this microtask/tick. */
  private flushScheduled = false;

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

    const backend = this.options.backend;

    // Create the WasmSyncClient in the worker if not already created
    if (!this.syncClientCreated && backend.createSyncClient) {
      await backend.createSyncClient(
        this.options.serverUrl,
        this.options.workspaceId,
        this.options.authToken,
      );
      if (this.options.sessionCode && backend.syncSetSessionCode) {
        await backend.syncSetSessionCode(this.options.sessionCode);
      }
      this.syncClientCreated = true;
    }

    // Get the WebSocket URL from Rust
    let url: string;
    if (backend.syncGetWsUrl) {
      url = await backend.syncGetWsUrl();
    } else {
      url = this.buildUrl();
    }

    console.log(`[UnifiedSyncTransport] Connecting to: ${url.replace(/token=[^&]+/, 'token=***')}`);
    this.connectionGeneration++;
    const gen = this.connectionGeneration;

    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";

    this.ws.onopen = async () => {
      console.log('[UnifiedSyncTransport] WebSocket opened');
      if (gen !== this.connectionGeneration) return;

      this.options.onStatusChange?.(true);

      // Notify Rust sync client that the WebSocket connected
      if (backend.syncOnConnected) {
        await backend.syncOnConnected();
        if (gen !== this.connectionGeneration) return;
        await this.drainAndSend();
      }

      // Resend focus list and re-request body sync after reconnect
      if (this.focusedFiles.size > 0) {
        if (backend.syncFocusFiles) {
          await backend.syncFocusFiles(Array.from(this.focusedFiles));
          await this.drainAndSend();
        }
        if (backend.syncBodyFiles) {
          await backend.syncBodyFiles(Array.from(this.focusedFiles));
          await this.drainAndSend();
        }
      }
    };

    this.ws.onmessage = (event) => {
      if (this.destroyed) return;

      // Successfully received a message — reset reconnect backoff
      this.reconnectAttempts = 0;

      // Buffer the message and schedule a batched flush.
      // All messages arriving in the same event-loop tick are collected
      // and sent to the worker in a single Comlink round-trip, reducing
      // queue contention that blocks interactive calls like getWorkspaceTree.
      if (typeof event.data === "string") {
        this.pendingTextMessages.push(event.data);
      } else {
        this.pendingBinaryMessages.push(new Uint8Array(event.data as ArrayBuffer));
      }

      if (!this.flushScheduled) {
        this.flushScheduled = true;
        setTimeout(() => this.flushPendingMessages(), 0);
      }
    };

    this.ws.onclose = (event) => {
      console.log(`[UnifiedSyncTransport] WebSocket closed: code=${event.code}, reason='${event.reason}', wasClean=${event.wasClean}`);
      this.ws = null;
      this.handshakeComplete = false;
      this.options.onStatusChange?.(false);

      // Notify Rust of disconnect
      if (backend.syncOnDisconnected) {
        backend.syncOnDisconnected().catch(e => {
          console.warn('[UnifiedSyncTransport] Error notifying disconnect:', e);
        });
      }

      if (!this.destroyed) {
        // Don't reconnect on fatal server errors (4000-4999 are application-level rejections)
        if (event.code >= 4000 && event.code < 5000) {
          console.error(`[UnifiedSyncTransport] Server rejected connection (code ${event.code}): ${event.reason}. Not reconnecting.`);
          const reason = event.reason || `Server rejected connection (code ${event.code})`;
          this.options.onError?.(
            `${reason}. The sync server may not support this protocol version. Check the server URL in Sync settings.`
          );
          return;
        }
        this.scheduleReconnect();
      }
    };

    this.ws.onerror = (e) => {
      console.error("[UnifiedSyncTransport] WebSocket error:", e);
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

    // Destroy the sync client in the worker
    if (this.syncClientCreated && this.options.backend.destroySyncClient) {
      this.options.backend.destroySyncClient().catch(e => {
        console.warn('[UnifiedSyncTransport] Error destroying sync client:', e);
      });
      this.syncClientCreated = false;
    }

    this.focusedFiles.clear();
    this.pendingBinaryMessages = [];
    this.pendingTextMessages = [];
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
   * Check if handshake is complete.
   */
  get isHandshakeComplete(): boolean {
    return this.handshakeComplete;
  }

  /**
   * Queue a local CRDT update for sending to the server.
   * Call this when local CRDT changes need to be synced.
   */
  async queueLocalUpdate(docId: string, data: Uint8Array): Promise<void> {
    console.log('[UnifiedSyncTransport] queueLocalUpdate: docId=', docId, 'data_len=', data.length, 'isConnected=', this.isConnected);
    const backend = this.options.backend;
    if (backend.syncQueueLocalUpdate) {
      await backend.syncQueueLocalUpdate(docId, data);
      await this.drainAndSend();
    } else {
      console.warn('[UnifiedSyncTransport] queueLocalUpdate: no syncQueueLocalUpdate on backend');
    }
  }

  /**
   * Focus on specific files for sync.
   */
  async focus(filePaths: string[]): Promise<void> {
    for (const filePath of filePaths) {
      this.focusedFiles.add(filePath);
    }

    const backend = this.options.backend;
    if (this.isConnected && backend.syncFocusFiles) {
      await backend.syncFocusFiles(filePaths);
      await this.drainAndSend();
    }
  }

  /**
   * Request body sync for specific files (lazy sync on demand).
   * Call this when opening a file to trigger its body doc sync.
   */
  async requestBodySync(filePaths: string[]): Promise<void> {
    const backend = this.options.backend;
    if (this.isConnected && backend.syncBodyFiles) {
      await backend.syncBodyFiles(filePaths);
      await this.drainAndSend();
    }
  }

  /**
   * Unfocus specific files.
   */
  async unfocus(filePaths: string[]): Promise<void> {
    const actuallyFocused = filePaths.filter((fp) => this.focusedFiles.has(fp));
    if (actuallyFocused.length === 0) return;

    for (const filePath of actuallyFocused) {
      this.focusedFiles.delete(filePath);
    }

    const backend = this.options.backend;
    if (this.isConnected && backend.syncUnfocusFiles) {
      await backend.syncUnfocusFiles(actuallyFocused);
      await this.drainAndSend();
    }
  }

  // =========================================================================
  // Internal: Message batching
  // =========================================================================

  /**
   * Flush all buffered WebSocket messages to the worker in batched calls.
   * Reduces 2N Comlink round-trips (N messages × inject + drain) to just 2-3
   * (one batch-inject for binary, one for text, one drain).
   */
  private async flushPendingMessages(): Promise<void> {
    this.flushScheduled = false;

    // Snapshot and clear the buffers so new messages arriving during the
    // async flush get queued into the next batch.
    const binaryBatch = this.pendingBinaryMessages;
    const textBatch = this.pendingTextMessages;
    this.pendingBinaryMessages = [];
    this.pendingTextMessages = [];

    if (binaryBatch.length === 0 && textBatch.length === 0) return;

    const backend = this.options.backend;

    // Inject binary messages
    if (binaryBatch.length > 0) {
      if (binaryBatch.length === 1 && backend.syncOnBinaryMessage) {
        await backend.syncOnBinaryMessage(binaryBatch[0]);
      } else if (backend.syncOnBinaryMessages) {
        await backend.syncOnBinaryMessages(binaryBatch);
      } else if (backend.syncOnBinaryMessage) {
        // Fallback: send one at a time (e.g. Tauri backend)
        for (const msg of binaryBatch) {
          await backend.syncOnBinaryMessage(msg);
        }
      }
    }

    // Inject text messages
    if (textBatch.length > 0) {
      if (textBatch.length === 1 && backend.syncOnTextMessage) {
        await backend.syncOnTextMessage(textBatch[0]);
      } else if (backend.syncOnTextMessages) {
        await backend.syncOnTextMessages(textBatch);
      } else if (backend.syncOnTextMessage) {
        // Fallback: send one at a time (e.g. Tauri backend)
        for (const msg of textBatch) {
          await backend.syncOnTextMessage(msg);
        }
      }
    }

    // Single drain for the entire batch
    await this.drainAndSend();
  }

  // =========================================================================
  // Internal: Drain outgoing messages and events from Rust
  // =========================================================================

  /**
   * Drain outgoing messages and events from the WasmSyncClient and send/dispatch them.
   */
  private async drainAndSend(): Promise<void> {
    const backend = this.options.backend;
    if (!backend.syncDrain) return;

    const { binary, text, events } = await backend.syncDrain();

    if (binary.length > 0 || text.length > 0) {
      console.log('[UnifiedSyncTransport] drainAndSend: binary=', binary.length, 'text=', text.length, 'events=', events.length, 'isConnected=', this.isConnected);
    }

    // Send binary messages
    for (const msg of binary) {
      this.safeSend(msg);
    }

    // Send text messages
    for (const msg of text) {
      this.safeSend(msg);
    }

    // Process events
    for (const eventJson of events) {
      this.handleSyncEvent(eventJson);
    }
  }

  /**
   * Send data on the WebSocket, guarding against CLOSING/CLOSED state.
   */
  private safeSend(data: Uint8Array | string): boolean {
    try {
      if (this.isConnected) {
        this.ws!.send(data);
        return true;
      }
    } catch (e) {
      console.warn("[UnifiedSyncTransport] Send failed:", e);
    }
    return false;
  }

  /**
   * Handle a JSON-serialized SyncEvent from Rust.
   */
  private handleSyncEvent(eventJson: string): void {
    try {
      const event = JSON.parse(eventJson);

      switch (event.type) {
        case 'statusChanged':
          if (event.status?.state === 'synced') {
            // Rust SyncSession only emits `synced` after metadata + pending body
            // bootstrap are complete.
            if (!this.workspaceSynced) {
              this.workspaceSynced = true;
              this.options.onWorkspaceSynced?.();
            }
          }
          break;

        case 'progress':
          this.options.onProgress?.(event.completed, event.total);
          break;

        case 'filesChanged':
          this.options.onFilesChanged?.(event.files ?? []);
          break;

        case 'bodyChanged':
          this.options.onBodyChanged?.(event.filePath ?? event.file_path);
          break;

        case 'error':
          console.error('[UnifiedSyncTransport] Sync error:', event.message);
          this.options.onError?.(event.message);
          break;

        // Special event from Rust: download snapshot (not a SyncEvent, but a SessionAction)
        case 'downloadSnapshot':
          this.handleDownloadSnapshot(event.workspaceId).catch(e => {
            console.error('[UnifiedSyncTransport] Snapshot download failed:', e);
          });
          break;

        // Control messages forwarded as events by SyncSession
        case 'syncComplete':
          this.options.onSyncComplete?.(event.filesSynced ?? 0);
          break;

        case 'focusListChanged':
          this.options.onFocusListChanged?.(event.files ?? []);
          break;

        case 'sessionJoined':
          this.options.onSessionJoined?.({
            joinCode: event.joinCode,
            workspaceId: event.workspaceId,
            readOnly: event.readOnly ?? false,
          });
          break;

        case 'peerJoined':
          this.options.onPeerJoined?.(event.guestId, event.peerCount ?? event.peer_count);
          break;

        case 'peerLeft':
          this.options.onPeerLeft?.(event.guestId, event.peerCount ?? event.peer_count);
          break;

        case 'sessionEnded':
          this.options.onSessionEnded?.();
          break;

        default:
          // Unknown event type — ignore
          break;
      }
    } catch (e) {
      console.warn('[UnifiedSyncTransport] Failed to parse sync event:', e, eventJson);
    }
  }

  // =========================================================================
  // Snapshot Download (HTTP, main thread)
  // =========================================================================

  /**
   * Convert server URL to HTTP URL for API calls.
   */
  private getHttpServerUrl(): string {
    return this.options.serverUrl
      .replace(/^wss:\/\//, "https://")
      .replace(/^ws:\/\//, "http://")
      .replace(/\/sync2$/, "")
      .replace(/\/sync$/, "");
  }

  /**
   * Download and import a workspace snapshot, then notify Rust.
   */
  private async handleDownloadSnapshot(workspaceId: string): Promise<void> {
    const httpUrl = this.getHttpServerUrl();
    const url = `${httpUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot`;
    const authToken = this.options.authToken;

    try {
      const response = await fetch(url, {
        headers: authToken ? { Authorization: `Bearer ${authToken}` } : {},
      });

      if (!response.ok) {
        console.warn(`[UnifiedSyncTransport] Snapshot download failed: ${response.status}`);
        // Still notify Rust so handshake continues
        if (this.options.backend.syncOnSnapshotImported) {
          await this.options.backend.syncOnSnapshotImported();
          await this.drainAndSend();
        }
        return;
      }

      const snapshot = await response.blob();

      if (snapshot && snapshot.size > 100) {
        const snapshotFile = new File(
          [snapshot],
          `snapshot-${workspaceId}.zip`,
          { type: "application/zip" },
        );

        const workspacePath = this.options.backend
          .getWorkspacePath()
          .replace(/\/index\.md$/, "")
          .replace(/\/README\.md$/, "");

        await this.options.backend.importFromZip(snapshotFile, workspacePath);
      }
    } catch (error) {
      console.error("[UnifiedSyncTransport] Download/import error:", error);
    }

    // Notify Rust that snapshot was imported (or failed)
    if (this.options.backend.syncOnSnapshotImported) {
      await this.options.backend.syncOnSnapshotImported();
      await this.drainAndSend();
    }
  }

  // =========================================================================
  // URL and Reconnection
  // =========================================================================

  /**
   * Build the WebSocket URL for /sync2 endpoint (fallback if Rust URL not available).
   */
  private buildUrl(): string {
    let url = this.options.serverUrl.replace(/\/sync$/, "/sync2");
    if (!url.endsWith("/sync2")) {
      url = url.replace(/\/$/, "") + "/sync2";
    }

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
