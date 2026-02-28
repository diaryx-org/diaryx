/**
 * Extism-based sync plugin wrapper.
 *
 * Wraps a loaded Extism plugin (diaryx_sync.wasm) and provides typed methods
 * matching the guest exports defined in `diaryx_sync_extism/src/lib.rs`.
 *
 * ## Architecture
 *
 * The plugin owns all CRDT state (WorkspaceCrdt, BodyDocManager, SyncSession)
 * inside the WASM sandbox. The host communicates through:
 * - **JSON exports** for lifecycle/commands (init, shutdown, handle_command)
 * - **Binary exports** for hot-path sync messages (handle_binary_message, etc.)
 *
 * Binary exports return an action envelope:
 * ```
 * [u16: num_actions]
 * for each action:
 *   [u8: action_type]   // 0=SendBinary, 1=SendText, 2=EmitEvent, 3=DownloadSnapshot
 *   [u32: payload_len]
 *   [payload_bytes]
 * ```
 */

import type { Plugin, PluginOutput } from '@extism/extism';

// ============================================================================
// Types
// ============================================================================

/** Manifest returned by the guest's `manifest()` export. */
export interface GuestManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  ui: unknown[];
  commands: string[];
}

/** Command request sent to `handle_command()`. */
export interface CommandRequest {
  command: string;
  params: unknown;
}

/** Command response from `handle_command()`. */
export interface CommandResponse {
  success: boolean;
  data?: unknown;
  error?: string;
}

/** Decoded action from the binary envelope. */
export type DecodedAction =
  | { type: 'SendBinary'; data: Uint8Array }
  | { type: 'SendText'; text: string }
  | { type: 'EmitEvent'; json: string }
  | { type: 'DownloadSnapshot'; workspaceId: string };

/** Drain result matching the WasmSyncClient interface. */
export interface DrainResult {
  binary: Uint8Array[];
  text: string[];
  events: string[];
}

// ============================================================================
// Binary envelope decoder
// ============================================================================

const ACTION_SEND_BINARY = 0;
const ACTION_SEND_TEXT = 1;
const ACTION_EMIT_EVENT = 2;
const ACTION_DOWNLOAD_SNAPSHOT = 3;

/**
 * Decode a binary action envelope returned by guest binary exports.
 */
function decodeActions(data: Uint8Array): DecodedAction[] {
  if (data.length < 2) return [];

  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const numActions = view.getUint16(0, true);
  let offset = 2;
  const actions: DecodedAction[] = [];

  for (let i = 0; i < numActions; i++) {
    if (offset >= data.length) break;

    const actionType = data[offset];
    offset += 1;

    if (offset + 4 > data.length) break;
    const payloadLen = view.getUint32(offset, true);
    offset += 4;

    if (offset + payloadLen > data.length) break;
    const payload = data.slice(offset, offset + payloadLen);
    offset += payloadLen;

    switch (actionType) {
      case ACTION_SEND_BINARY:
        actions.push({ type: 'SendBinary', data: payload });
        break;
      case ACTION_SEND_TEXT:
        actions.push({ type: 'SendText', text: new TextDecoder().decode(payload) });
        break;
      case ACTION_EMIT_EVENT:
        actions.push({ type: 'EmitEvent', json: new TextDecoder().decode(payload) });
        break;
      case ACTION_DOWNLOAD_SNAPSHOT:
        actions.push({
          type: 'DownloadSnapshot',
          workspaceId: new TextDecoder().decode(payload),
        });
        break;
      default:
        console.warn(`[ExtismSyncPlugin] Unknown action type: ${actionType}`);
    }
  }

  return actions;
}

/**
 * Convert decoded actions into a DrainResult, separating messages from events.
 */
function actionsToDrainResult(actions: DecodedAction[]): DrainResult {
  const result: DrainResult = { binary: [], text: [], events: [] };

  for (const action of actions) {
    switch (action.type) {
      case 'SendBinary':
        result.binary.push(action.data);
        break;
      case 'SendText':
        result.text.push(action.text);
        break;
      case 'EmitEvent':
        result.events.push(action.json);
        break;
      case 'DownloadSnapshot':
        // Emit as a special event for the transport to handle
        result.events.push(JSON.stringify({
          type: 'downloadSnapshot',
          workspaceId: action.workspaceId,
        }));
        break;
    }
  }

  return result;
}

// ============================================================================
// ExtismSyncPlugin
// ============================================================================

/**
 * Wraps an Extism plugin providing the sync protocol interface.
 *
 * Implements the same contract as WasmSyncClient but delegates to
 * the diaryx_sync Extism guest instead of inline Rust WASM code.
 */
export class ExtismSyncPlugin {
  private plugin: Plugin;
  private _manifest: GuestManifest | null = null;
  private callChain: Promise<void> = Promise.resolve();

  constructor(plugin: Plugin) {
    this.plugin = plugin;
  }

  // =========================================================================
  // Lifecycle
  // =========================================================================

  /** Get the plugin manifest. Cached after first call. */
  async getManifest(): Promise<GuestManifest> {
    if (this._manifest) return this._manifest;
    const output = await this.callJson('manifest', '');
    this._manifest = output as GuestManifest;
    return this._manifest;
  }

  /** Initialize the sync plugin with workspace context. */
  async init(params: {
    workspaceId: string;
    workspaceRoot?: string;
    writeToDisk: boolean;
  }): Promise<void> {
    // Guest expects snake_case keys matching Rust InitParams struct
    await this.callJson('init', JSON.stringify({
      workspace_id: params.workspaceId,
      workspace_root: params.workspaceRoot,
      write_to_disk: params.writeToDisk,
    }));
  }

  /** Shut down the sync plugin, persisting state. */
  async shutdown(): Promise<void> {
    await this.callJson('shutdown', '');
  }

  /** Close the underlying Extism plugin. */
  async close(): Promise<void> {
    await this.serializeCall(async () => {
      await this.plugin.close();
    });
  }

  // =========================================================================
  // Commands (JSON protocol)
  // =========================================================================

  /** Send a command to the sync plugin. */
  async handleCommand(command: string, params: unknown = {}): Promise<CommandResponse> {
    const request: CommandRequest = { command, params };
    const output = await this.callJson('handle_command', JSON.stringify(request));
    return output as CommandResponse;
  }

  // =========================================================================
  // Events (JSON protocol)
  // =========================================================================

  /** Notify the plugin of a filesystem event. */
  async onEvent(eventType: string, payload: unknown = {}): Promise<void> {
    await this.callJson('on_event', JSON.stringify({ event_type: eventType, payload }));
  }

  // =========================================================================
  // Binary hot-path exports
  // =========================================================================

  /** Process an incoming binary WebSocket message. */
  async handleBinaryMessage(data: Uint8Array): Promise<DrainResult> {
    const output = await this.callPlugin('handle_binary_message', data);
    return this.decodeBinaryOutput(output);
  }

  /** Process an incoming text WebSocket message. */
  async handleTextMessage(text: string): Promise<DrainResult> {
    const output = await this.callPlugin('handle_text_message', text);
    return this.decodeBinaryOutput(output);
  }

  /** Notify the plugin that WebSocket connected. */
  async onConnected(serverUrl: string): Promise<DrainResult> {
    const output = await this.callPlugin('on_connected', serverUrl);
    return this.decodeBinaryOutput(output);
  }

  /** Notify the plugin that WebSocket disconnected. */
  async onDisconnected(reason: string = ''): Promise<DrainResult> {
    const output = await this.callPlugin('on_disconnected', reason);
    return this.decodeBinaryOutput(output);
  }

  /** Queue a local CRDT update for sync. */
  async queueLocalUpdate(docId: string, data: Uint8Array): Promise<DrainResult> {
    // Guest expects JSON: {"doc_id": "...", "data": "base64..."}
    const b64 = uint8ArrayToBase64(data);
    const output = await this.callPlugin(
      'queue_local_update',
      JSON.stringify({ doc_id: docId, data: b64 }),
    );
    return this.decodeBinaryOutput(output);
  }

  /** Notify the plugin that a snapshot was imported. */
  async onSnapshotImported(): Promise<DrainResult> {
    const output = await this.callPlugin('on_snapshot_imported', '');
    return this.decodeBinaryOutput(output);
  }

  /** Request body sync for specific files. */
  async syncBodyFiles(files: string[]): Promise<DrainResult> {
    // Guest expects JSON: {"file_paths": [...]}
    const output = await this.callPlugin(
      'sync_body_files',
      JSON.stringify({ file_paths: files }),
    );
    return this.decodeBinaryOutput(output);
  }

  // =========================================================================
  // Helpers
  // =========================================================================

  private async callJson(funcName: string, input: string): Promise<unknown> {
    const output = await this.callPlugin(funcName, input);
    if (!output) return null;
    const text = output.text();
    if (!text || text === '""' || text === '') return null;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }

  private decodeBinaryOutput(output: PluginOutput | null): DrainResult {
    if (!output) return { binary: [], text: [], events: [] };
    const bytes = output.bytes();
    if (bytes.length === 0) return { binary: [], text: [], events: [] };
    const actions = decodeActions(bytes);
    return actionsToDrainResult(actions);
  }

  /**
   * Extism plugins are not re-entrant. Serialize calls so browser-side event
   * fan-out can't trigger overlapping guest invocations that panic on RefCell
   * re-borrows in the WASM plugin state.
   */
  private async serializeCall<T>(operation: () => Promise<T>): Promise<T> {
    const previous = this.callChain;
    let release: () => void = () => {};
    this.callChain = new Promise<void>((resolve) => {
      release = resolve;
    });

    await previous.catch(() => {});

    try {
      return await operation();
    } finally {
      release();
    }
  }

  private async callPlugin(
    funcName: string,
    input: string | Uint8Array,
  ): Promise<PluginOutput | null> {
    return this.serializeCall(() => this.plugin.call(funcName, input));
  }
}

// ============================================================================
// Helpers
// ============================================================================

function uint8ArrayToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}
