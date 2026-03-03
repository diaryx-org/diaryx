/**
 * Plugin Sync Adapter — manages a WebSocket connection and delegates
 * sync protocol processing to the Extism sync guest plugin.
 *
 * Replaces the Rust-owned WebSocket transport (WasmSyncTransport) with
 * a TypeScript-owned WebSocket + Extism binary calls. The sync protocol
 * logic stays in Rust (inside the Extism guest); this adapter just owns
 * the transport and decodes the plugin's binary action envelopes.
 */

import type { BrowserExtismPlugin } from "$lib/plugins/extismBrowserLoader";
import type { Backend, FileSystemEvent } from "$lib/backend/interface";

// ============================================================================
// Binary action protocol (mirrors diaryx_sync_extism::binary_protocol)
// ============================================================================

const ACTION_SEND_BINARY = 0;
const ACTION_SEND_TEXT = 1;
const ACTION_EMIT_EVENT = 2;
const ACTION_DOWNLOAD_SNAPSHOT = 3;

interface DecodedAction {
  type: "SendBinary" | "SendText" | "EmitEvent" | "DownloadSnapshot";
  data: Uint8Array | string;
}

function decodeActions(buf: Uint8Array): DecodedAction[] {
  if (buf.length < 2) return [];

  const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  const numActions = view.getUint16(0, true);
  let offset = 2;
  const actions: DecodedAction[] = [];

  for (let i = 0; i < numActions; i++) {
    if (offset >= buf.length) break;
    const actionType = buf[offset];
    offset += 1;

    if (offset + 4 > buf.length) break;
    const payloadLen = view.getUint32(offset, true);
    offset += 4;

    if (offset + payloadLen > buf.length) break;
    const payload = buf.slice(offset, offset + payloadLen);
    offset += payloadLen;

    switch (actionType) {
      case ACTION_SEND_BINARY:
        actions.push({ type: "SendBinary", data: payload });
        break;
      case ACTION_SEND_TEXT:
        actions.push({
          type: "SendText",
          data: new TextDecoder().decode(payload),
        });
        break;
      case ACTION_EMIT_EVENT:
        actions.push({
          type: "EmitEvent",
          data: new TextDecoder().decode(payload),
        });
        break;
      case ACTION_DOWNLOAD_SNAPSHOT:
        actions.push({
          type: "DownloadSnapshot",
          data: new TextDecoder().decode(payload),
        });
        break;
      default:
        console.warn(`[PluginSyncAdapter] Unknown action type: ${actionType}`);
    }
  }

  return actions;
}

// ============================================================================
// SyncEvent → FileSystemEvent conversion
// ============================================================================

/**
 * Convert a SyncEvent (from the Rust sync session, serialized as camelCase JSON)
 * to a FileSystemEvent (consumed by the host event subscription pipeline).
 */
function syncEventToFileSystemEvent(
  syncEvent: any,
): FileSystemEvent | null {
  switch (syncEvent.type) {
    case "statusChanged": {
      const state = syncEvent.status?.state;
      return {
        type: "SyncStatusChanged",
        status: state ?? "unknown",
      } as FileSystemEvent;
    }
    case "progress":
      return {
        type: "SyncProgress",
        completed: syncEvent.completed ?? 0,
        total: syncEvent.total ?? 0,
      } as FileSystemEvent;
    case "filesChanged":
      return {
        type: "SyncCompleted",
        doc_name: "",
        files_synced: Array.isArray(syncEvent.files)
          ? syncEvent.files.length
          : 0,
      } as FileSystemEvent;
    case "bodyChanged":
      // BodyChanged events are handled specially — we need to read the
      // body content from the plugin and emit ContentsChanged.
      // Return null here; the caller handles this case.
      return null;
    case "error":
      return {
        type: "SyncStatusChanged",
        status: "error",
        error: syncEvent.message ?? "Unknown error",
      } as FileSystemEvent;
    case "peerJoined":
      return {
        type: "PeerJoined",
        peer_count: syncEvent.peerCount ?? 0,
      } as FileSystemEvent;
    case "peerLeft":
      return {
        type: "PeerLeft",
        peer_count: syncEvent.peerCount ?? 0,
      } as FileSystemEvent;
    case "syncComplete":
      return {
        type: "SyncCompleted",
        doc_name: "",
        files_synced: syncEvent.filesSynced ?? 0,
      } as FileSystemEvent;
    case "focusListChanged":
      return {
        type: "FocusListChanged",
        files: syncEvent.files ?? [],
      } as FileSystemEvent;
    default:
      console.warn(
        `[PluginSyncAdapter] Unknown SyncEvent type: ${syncEvent.type}`,
      );
      return null;
  }
}

// ============================================================================
// PluginSyncAdapter
// ============================================================================

export interface PluginSyncAdapterOptions {
  /** The Extism sync plugin instance. */
  syncPlugin: BrowserExtismPlugin;
  /** The backend (for emitting events and importing snapshots). */
  backend: Backend;
  /** Auth token for snapshot download. */
  getAuthToken: () => string | undefined;
  /** HTTP server URL (for snapshot downloads). */
  serverHttpUrl: string;
}

export class PluginSyncAdapter {
  private ws: WebSocket | null = null;
  private syncPlugin: BrowserExtismPlugin;
  private backend: Backend;
  private getAuthToken: () => string | undefined;
  private serverHttpUrl: string;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempt = 0;
  private maxReconnectDelay = 30_000;
  private shouldReconnect = false;
  private _connected = false;

  constructor(options: PluginSyncAdapterOptions) {
    this.syncPlugin = options.syncPlugin;
    this.backend = options.backend;
    this.getAuthToken = options.getAuthToken;
    this.serverHttpUrl = options.serverHttpUrl;
  }

  /**
   * Connect to the sync server and start the sync session.
   */
  async connect(
    wsUrl: string,
    workspaceId: string,
    authToken?: string,
    sessionCode?: string,
  ): Promise<void> {
    this.shouldReconnect = true;

    // Initialize the sync plugin
    await this.syncPlugin.callCommand("init", {
      workspace_id: workspaceId,
      write_to_disk: true,
    });

    // Build the WebSocket URL
    let url = `${wsUrl}/sync2`;
    const params: string[] = [];
    if (authToken) params.push(`token=${encodeURIComponent(authToken)}`);
    if (sessionCode)
      params.push(`session=${encodeURIComponent(sessionCode)}`);
    if (params.length > 0) url += `?${params.join("&")}`;

    this.openWebSocket(url, workspaceId);
  }

  /**
   * Disconnect from the sync server.
   */
  async disconnect(): Promise<void> {
    this.shouldReconnect = false;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    if (this.ws) {
      this.ws.onclose = null;
      this.ws.onmessage = null;
      this.ws.onerror = null;
      this.ws.close();
      this.ws = null;
    }

    if (this._connected) {
      this._connected = false;
      // Notify plugin of disconnection
      const response = await this.syncPlugin.callBinary(
        "on_disconnected",
        new TextEncoder().encode(""),
      );
      if (response) {
        await this.executeActions(response);
      }
    }
  }

  /**
   * Focus on specific files for body sync.
   */
  async focusFiles(files: string[]): Promise<void> {
    const response = await this.syncPlugin.callBinary(
      "sync_body_files",
      new TextEncoder().encode(JSON.stringify({ file_paths: files })),
    );
    if (response) {
      await this.executeActions(response);
    }
  }

  /**
   * Request body sync for specific files.
   */
  async requestBodySync(files: string[]): Promise<void> {
    // sync_body_files handles both focus and requesting sync
    await this.focusFiles(files);
  }

  /**
   * Notify the plugin that a snapshot was imported.
   */
  async notifySnapshotImported(): Promise<void> {
    const response = await this.syncPlugin.callBinary(
      "on_snapshot_imported",
      new TextEncoder().encode(""),
    );
    if (response) {
      await this.executeActions(response);
    }
  }

  /**
   * Queue a local CRDT update to be sent to the server.
   */
  async queueLocalUpdate(docId: string, data: Uint8Array): Promise<void> {
    // Convert to base64 for the JSON input format
    let binary = "";
    for (let i = 0; i < data.length; i++) {
      binary += String.fromCharCode(data[i]);
    }
    const base64 = btoa(binary);

    const input = JSON.stringify({ doc_id: docId, data: base64 });
    const response = await this.syncPlugin.callBinary(
      "queue_local_update",
      new TextEncoder().encode(input),
    );
    if (response) {
      await this.executeActions(response);
    }
  }

  // ==========================================================================
  // Private
  // ==========================================================================

  private openWebSocket(url: string, workspaceId: string): void {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;

    ws.onopen = async () => {
      this._connected = true;
      this.reconnectAttempt = 0;

      // Notify plugin of connection
      const configJson = JSON.stringify({ workspace_id: workspaceId });
      const response = await this.syncPlugin.callBinary(
        "on_connected",
        new TextEncoder().encode(configJson),
      );
      if (response) {
        await this.executeActions(response);
      }
    };

    ws.onmessage = async (event: MessageEvent) => {
      try {
        if (event.data instanceof ArrayBuffer) {
          const bytes = new Uint8Array(event.data);
          const response = await this.syncPlugin.callBinary(
            "handle_binary_message",
            bytes,
          );
          if (response) {
            await this.executeActions(response);
          }
        } else if (typeof event.data === "string") {
          const response = await this.syncPlugin.callBinary(
            "handle_text_message",
            new TextEncoder().encode(event.data),
          );
          if (response) {
            await this.executeActions(response);
          }
        }
      } catch (e) {
        console.error("[PluginSyncAdapter] Error handling message:", e);
      }
    };

    ws.onerror = (event) => {
      console.error("[PluginSyncAdapter] WebSocket error:", event);
    };

    ws.onclose = async () => {
      this._connected = false;

      // Notify plugin of disconnection
      try {
        const response = await this.syncPlugin.callBinary(
          "on_disconnected",
          new TextEncoder().encode(""),
        );
        if (response) {
          await this.executeActions(response);
        }
      } catch (e) {
        console.warn(
          "[PluginSyncAdapter] Error notifying plugin of disconnect:",
          e,
        );
      }

      // Schedule reconnect if desired
      if (this.shouldReconnect) {
        this.scheduleReconnect(url, workspaceId);
      }
    };
  }

  private scheduleReconnect(url: string, workspaceId: string): void {
    this.reconnectAttempt++;
    const delay = Math.min(
      1000 * Math.pow(1.5, this.reconnectAttempt - 1),
      this.maxReconnectDelay,
    );
    console.log(
      `[PluginSyncAdapter] Reconnecting in ${Math.round(delay)}ms (attempt ${this.reconnectAttempt})`,
    );

    // Emit reconnecting status
    this.backend.emitFileSystemEvent?.({
      type: "SyncStatusChanged",
      status: "connecting",
    } as FileSystemEvent);

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.shouldReconnect) {
        this.openWebSocket(url, workspaceId);
      }
    }, delay);
  }

  private async executeActions(response: Uint8Array): Promise<void> {
    const actions = decodeActions(response);

    for (const action of actions) {
      switch (action.type) {
        case "SendBinary":
          if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(action.data as Uint8Array);
          }
          break;

        case "SendText":
          if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(action.data as string);
          }
          break;

        case "EmitEvent": {
          try {
            const syncEvent = JSON.parse(action.data as string);

            // Handle BodyChanged specially — read body content from plugin
            if (syncEvent.type === "bodyChanged" && syncEvent.filePath) {
              await this.handleBodyChanged(syncEvent.filePath);
              break;
            }

            const fsEvent = syncEventToFileSystemEvent(syncEvent);
            if (fsEvent) {
              this.backend.emitFileSystemEvent?.(fsEvent);
            }
          } catch (e) {
            console.error(
              "[PluginSyncAdapter] Error processing EmitEvent:",
              e,
            );
          }
          break;
        }

        case "DownloadSnapshot": {
          const workspaceId = action.data as string;
          await this.handleDownloadSnapshot(workspaceId);
          break;
        }
      }
    }
  }

  /**
   * Handle a BodyChanged event by reading the body content from the plugin
   * and emitting a ContentsChanged FileSystemEvent.
   */
  private async handleBodyChanged(filePath: string): Promise<void> {
    try {
      const response = await this.syncPlugin.callTypedCommand({
        type: "GetBodyContent",
        params: { path: filePath },
      });
      const body =
        response && typeof response === "object" && "data" in response
          ? (response as any).data ?? ""
          : "";
      this.backend.emitFileSystemEvent?.({
        type: "ContentsChanged",
        path: filePath,
        body,
      } as FileSystemEvent);
    } catch (e) {
      console.warn(
        `[PluginSyncAdapter] Failed to read body for ${filePath}:`,
        e,
      );
      // Still emit the event so the bridge knows the file changed
      this.backend.emitFileSystemEvent?.({
        type: "ContentsChanged",
        path: filePath,
        body: "",
      } as FileSystemEvent);
    }
  }

  /**
   * Handle a DownloadSnapshot action by fetching the snapshot zip
   * from the server and importing it into the backend.
   */
  private async handleDownloadSnapshot(workspaceId: string): Promise<void> {
    try {
      const token = this.getAuthToken();
      if (!token) {
        console.error(
          "[PluginSyncAdapter] No auth token for snapshot download",
        );
        return;
      }

      const url = `${this.serverHttpUrl}/api/workspaces/${encodeURIComponent(workspaceId)}/snapshot?include_attachments=true`;
      const resp = await fetch(url, {
        headers: { Authorization: `Bearer ${token}` },
      });

      if (!resp.ok) {
        console.error(
          `[PluginSyncAdapter] Snapshot download failed: ${resp.status}`,
        );
        return;
      }

      const blob = await resp.blob();
      if (blob.size > 100) {
        const file = new File([blob], "snapshot.zip", {
          type: "application/zip",
        });
        await this.backend.importFromZip?.(file);
      }

      // Notify plugin that snapshot was imported
      await this.notifySnapshotImported();
    } catch (e) {
      console.error("[PluginSyncAdapter] Snapshot download error:", e);
    }
  }
}
