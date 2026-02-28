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
import { proxyFetch } from "../backend/proxyFetch";
import {
  getSyncWsHandlerFactory,
  type SyncWsHandler,
  type SyncWsDrainResult,
  type SyncWsRequest,
} from "../sync/syncWsRegistry";

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
  /** Optional plugin id used to resolve a registered SyncWsHandler factory. */
  syncPluginId?: string;
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

async function createLegacyBackendSyncHandler(
  options: UnifiedSyncTransportOptions,
): Promise<SyncWsHandler> {
  const { backend } = options;

  if (backend.createSyncClient) {
    await backend.createSyncClient(
      options.serverUrl,
      options.workspaceId,
      options.authToken,
    );
    if (options.sessionCode && backend.syncSetSessionCode) {
      await backend.syncSetSessionCode(options.sessionCode);
    }
  }

  return {
    async handle(request: SyncWsRequest): Promise<void> {
      switch (request.type) {
        case "connected":
          if (backend.syncOnConnected) {
            await backend.syncOnConnected();
          }
          break;
        case "disconnected":
          if (backend.syncOnDisconnected) {
            await backend.syncOnDisconnected();
          }
          break;
        case "incoming_binary":
          if (backend.syncOnBinaryMessage) {
            await backend.syncOnBinaryMessage(request.data);
          }
          break;
        case "incoming_text":
          if (backend.syncOnTextMessage) {
            await backend.syncOnTextMessage(request.text);
          }
          break;
        case "local_update":
          if (backend.syncQueueLocalUpdate) {
            await backend.syncQueueLocalUpdate(request.docId, request.data);
          }
          break;
        case "focus":
          if (backend.syncFocusFiles) {
            await backend.syncFocusFiles(request.files);
          }
          break;
        case "unfocus":
          if (backend.syncUnfocusFiles) {
            await backend.syncUnfocusFiles(request.files);
          }
          break;
        case "request_body":
          if (backend.syncBodyFiles) {
            await backend.syncBodyFiles(request.files);
          }
          break;
        case "snapshot_imported":
          if (backend.syncOnSnapshotImported) {
            await backend.syncOnSnapshotImported();
          }
          break;
        default:
          break;
      }
    },

    async drain(): Promise<SyncWsDrainResult> {
      if (backend.syncDrain) {
        return backend.syncDrain();
      }
      return { binary: [], text: [], events: [] };
    },

    async destroy(): Promise<void> {
      if (backend.destroySyncClient) {
        await backend.destroySyncClient();
      }
    },
  };
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

  /** Active sync handler implementation (plugin or backend compatibility). */
  private syncHandler: SyncWsHandler | null = null;
  /** True when using backend compatibility mode instead of a registered plugin handler. */
  private usingLegacyBackendHandler = false;
  /** In-flight handler initialization promise (dedupes concurrent init). */
  private syncHandlerInitPromise: Promise<void> | null = null;

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

    await this.ensureSyncHandler();

    // Get the WebSocket URL from backend (legacy mode) or build locally.
    let url: string;
    const syncGetWsUrl = this.options.backend.syncGetWsUrl;
    if (this.usingLegacyBackendHandler && syncGetWsUrl) {
      url = await syncGetWsUrl.call(this.options.backend);
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

      if (this.syncHandler) {
        await this.syncHandler.handle({
          type: "connected",
          serverUrl: this.options.serverUrl,
        });
        if (gen !== this.connectionGeneration) return;
        await this.drainAndSend();
      }

      // Resend focus list and re-request body sync after reconnect
      if (this.focusedFiles.size > 0) {
        if (this.syncHandler) {
          await this.syncHandler.handle({
            type: "focus",
            files: Array.from(this.focusedFiles),
          });
          await this.drainAndSend();
          await this.syncHandler.handle({
            type: "request_body",
            files: Array.from(this.focusedFiles),
          });
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

      // Notify sync handler of disconnect
      if (this.syncHandler) {
        this.syncHandler
          .handle({
            type: "disconnected",
            reason: event.reason || `code=${event.code}`,
          })
          .then(() => this.drainAndSend())
          .catch((e) => {
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

    if (this.syncHandler?.destroy) {
      Promise.resolve(this.syncHandler.destroy()).catch((e) => {
        console.warn('[UnifiedSyncTransport] Error destroying sync handler:', e);
      });
    }
    this.syncHandler = null;
    this.syncHandlerInitPromise = null;
    this.usingLegacyBackendHandler = false;

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
    await this.ensureSyncHandler();
    if (this.syncHandler) {
      await this.syncHandler.handle({ type: "local_update", docId, data });
      await this.drainAndSend();
      return;
    }
    console.warn('[UnifiedSyncTransport] queueLocalUpdate: no sync handler available');
  }

  /**
   * Focus on specific files for sync.
   */
  async focus(filePaths: string[]): Promise<void> {
    for (const filePath of filePaths) {
      this.focusedFiles.add(filePath);
    }

    await this.ensureSyncHandler();
    if (this.isConnected && this.syncHandler) {
      await this.syncHandler.handle({ type: "focus", files: filePaths });
      await this.drainAndSend();
    }
  }

  /**
   * Request body sync for specific files (lazy sync on demand).
   * Call this when opening a file to trigger its body doc sync.
   */
  async requestBodySync(filePaths: string[]): Promise<void> {
    await this.ensureSyncHandler();
    if (this.isConnected && this.syncHandler) {
      await this.syncHandler.handle({ type: "request_body", files: filePaths });
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

    await this.ensureSyncHandler();
    if (this.isConnected && this.syncHandler) {
      await this.syncHandler.handle({ type: "unfocus", files: actuallyFocused });
      await this.drainAndSend();
    }
  }

  private async ensureSyncHandler(): Promise<void> {
    if (this.syncHandler) return;
    if (this.syncHandlerInitPromise) {
      await this.syncHandlerInitPromise;
      return;
    }

    this.syncHandlerInitPromise = (async () => {
      const pluginId = this.options.syncPluginId;
      if (pluginId) {
        const factory = getSyncWsHandlerFactory(pluginId);
        if (factory) {
          this.syncHandler = await factory({
            pluginId,
            backend: this.options.backend,
            serverUrl: this.options.serverUrl,
            workspaceId: this.options.workspaceId,
            writeToDisk: this.options.writeToDisk,
            authToken: this.options.authToken,
            sessionCode: this.options.sessionCode,
          });
          this.usingLegacyBackendHandler = false;
          return;
        }
      }

      this.syncHandler = await createLegacyBackendSyncHandler(this.options);
      this.usingLegacyBackendHandler = true;
    })();

    try {
      await this.syncHandlerInitPromise;
    } finally {
      this.syncHandlerInitPromise = null;
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

    await this.ensureSyncHandler();
    if (!this.syncHandler) return;

    // Inject binary messages
    for (const msg of binaryBatch) {
      await this.syncHandler.handle({ type: "incoming_binary", data: msg });
    }

    // Inject text messages
    for (const msg of textBatch) {
      await this.syncHandler.handle({ type: "incoming_text", text: msg });
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
    if (!this.syncHandler) return;

    const { binary, text, events } = await this.syncHandler.drain();

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
      const response = await proxyFetch(url, {
        headers: authToken ? { Authorization: `Bearer ${authToken}` } : {},
      });

      if (!response.ok) {
        console.warn(`[UnifiedSyncTransport] Snapshot download failed: ${response.status}`);
        // Still notify Rust so handshake continues
        if (this.syncHandler) {
          await this.syncHandler.handle({ type: "snapshot_imported" });
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
    if (this.syncHandler) {
      await this.syncHandler.handle({ type: "snapshot_imported" });
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
