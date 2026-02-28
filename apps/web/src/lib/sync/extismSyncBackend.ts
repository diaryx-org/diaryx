/**
 * Extism sync adapter.
 *
 * Bridges the Extism guest plugin to:
 * 1) Legacy backend sync method names (for compatibility), and
 * 2) The new SyncWsHandler request/drain interface.
 */

import type { Backend } from '../backend/interface';
import type { ExtismSyncPlugin, DrainResult } from './extismSyncPlugin';
import type {
  SyncWsRequest,
  SyncWsHandler,
  SyncWsHandlerFactory,
} from './syncWsRegistry';

/**
 * Subset of the Backend interface used by legacy sync transport code.
 */
export interface SyncBackendMethods {
  createSyncClient(serverUrl: string, workspaceId: string, authToken?: string): Promise<void>;
  destroySyncClient(): Promise<void>;
  syncGetWsUrl(): Promise<string>;
  syncSetSessionCode(code: string): Promise<void>;
  syncOnConnected(): Promise<void>;
  syncOnBinaryMessage(data: Uint8Array): Promise<void>;
  syncOnBinaryMessages(messages: Uint8Array[]): Promise<void>;
  syncOnTextMessage(text: string): Promise<void>;
  syncOnTextMessages(messages: string[]): Promise<void>;
  syncOnDisconnected(): Promise<void>;
  syncOnSnapshotImported(): Promise<void>;
  syncQueueLocalUpdate(docId: string, data: Uint8Array): Promise<void>;
  syncDrain(): Promise<DrainResult>;
  syncFocusFiles(files: string[]): Promise<void>;
  syncUnfocusFiles(files: string[]): Promise<void>;
  syncBodyFiles(files: string[]): Promise<void>;
}

/**
 * Extism adapter that supports both legacy backend methods and SyncWsHandler.
 */
export class ExtismSyncBackendAdapter
  implements SyncBackendMethods, SyncWsHandler
{
  private plugin: ExtismSyncPlugin;
  private serverUrl = '';
  private authToken?: string;
  private sessionCode?: string;
  private readonly writeToDisk: boolean;
  private pendingDrain: DrainResult = { binary: [], text: [], events: [] };

  constructor(plugin: ExtismSyncPlugin, options: { writeToDisk?: boolean } = {}) {
    this.plugin = plugin;
    this.writeToDisk = options.writeToDisk ?? true;
  }

  async createSyncClient(
    serverUrl: string,
    workspaceId: string,
    authToken?: string,
  ): Promise<void> {
    this.serverUrl = serverUrl;
    this.authToken = authToken;

    await this.plugin.init({
      workspaceId,
      writeToDisk: this.writeToDisk,
    });
  }

  async destroySyncClient(): Promise<void> {
    await this.plugin.shutdown();
    this.pendingDrain = { binary: [], text: [], events: [] };
  }

  async syncGetWsUrl(): Promise<string> {
    let url = this.serverUrl.replace(/\/sync$/, '/sync2');
    if (!url.endsWith('/sync2')) {
      url = url.replace(/\/$/, '') + '/sync2';
    }

    const params = new URLSearchParams();
    if (this.authToken) params.set('token', this.authToken);
    if (this.sessionCode) params.set('session', this.sessionCode);

    const qs = params.toString();
    return qs ? `${url}?${qs}` : url;
  }

  async syncSetSessionCode(code: string): Promise<void> {
    this.sessionCode = code;
    await this.plugin.handleCommand('set_session_code', { code });
  }

  async syncOnConnected(): Promise<void> {
    const result = await this.plugin.onConnected(this.serverUrl);
    this.accumulateDrain(result);
  }

  async syncOnBinaryMessage(data: Uint8Array): Promise<void> {
    const result = await this.plugin.handleBinaryMessage(data);
    this.accumulateDrain(result);
  }

  async syncOnBinaryMessages(messages: Uint8Array[]): Promise<void> {
    for (const msg of messages) {
      const result = await this.plugin.handleBinaryMessage(msg);
      this.accumulateDrain(result);
    }
  }

  async syncOnTextMessage(text: string): Promise<void> {
    const result = await this.plugin.handleTextMessage(text);
    this.accumulateDrain(result);
  }

  async syncOnTextMessages(messages: string[]): Promise<void> {
    for (const msg of messages) {
      const result = await this.plugin.handleTextMessage(msg);
      this.accumulateDrain(result);
    }
  }

  async syncOnDisconnected(): Promise<void> {
    const result = await this.plugin.onDisconnected();
    this.accumulateDrain(result);
  }

  async syncOnSnapshotImported(): Promise<void> {
    const result = await this.plugin.onSnapshotImported();
    this.accumulateDrain(result);
  }

  async syncQueueLocalUpdate(docId: string, data: Uint8Array): Promise<void> {
    const result = await this.plugin.queueLocalUpdate(docId, data);
    this.accumulateDrain(result);
  }

  async syncDrain(): Promise<DrainResult> {
    const out = this.pendingDrain;
    this.pendingDrain = { binary: [], text: [], events: [] };
    return out;
  }

  async syncFocusFiles(files: string[]): Promise<void> {
    this.pendingDrain.text.push(JSON.stringify({ type: 'focus', files }));
  }

  async syncUnfocusFiles(files: string[]): Promise<void> {
    this.pendingDrain.text.push(JSON.stringify({ type: 'unfocus', files }));
  }

  async syncBodyFiles(files: string[]): Promise<void> {
    const result = await this.plugin.syncBodyFiles(files);
    this.accumulateDrain(result);
  }

  async handle(request: SyncWsRequest): Promise<void> {
    switch (request.type) {
      case 'connected':
        this.serverUrl = request.serverUrl;
        await this.syncOnConnected();
        break;
      case 'disconnected':
        await this.syncOnDisconnected();
        break;
      case 'incoming_binary':
        await this.syncOnBinaryMessage(request.data);
        break;
      case 'incoming_text':
        await this.syncOnTextMessage(request.text);
        break;
      case 'local_update':
        await this.syncQueueLocalUpdate(request.docId, request.data);
        break;
      case 'focus':
        await this.syncFocusFiles(request.files);
        break;
      case 'unfocus':
        await this.syncUnfocusFiles(request.files);
        break;
      case 'request_body':
        await this.syncBodyFiles(request.files);
        break;
      case 'snapshot_imported':
        await this.syncOnSnapshotImported();
        break;
      default:
        break;
    }
  }

  async drain(): Promise<DrainResult> {
    return this.syncDrain();
  }

  async destroy(): Promise<void> {
    await this.destroySyncClient();
  }

  private accumulateDrain(result: DrainResult): void {
    this.pendingDrain.binary.push(...result.binary);
    this.pendingDrain.text.push(...result.text);
    this.pendingDrain.events.push(...result.events);
  }
}

/**
 * Create a SyncWsHandler factory bound to a loaded Extism sync plugin.
 */
export function createExtismSyncWsHandlerFactory(
  plugin: ExtismSyncPlugin,
): SyncWsHandlerFactory {
  return async (options) => {
    const adapter = new ExtismSyncBackendAdapter(plugin, {
      writeToDisk: options.writeToDisk,
    });
    await adapter.createSyncClient(
      options.serverUrl,
      options.workspaceId,
      options.authToken,
    );
    if (options.sessionCode) {
      await adapter.syncSetSessionCode(options.sessionCode);
    }
    return adapter;
  };
}

// ============================================================================
// Legacy compatibility: sync overlay proxy
// ============================================================================

const SYNC_METHOD_NAMES = new Set<string>([
  'createSyncClient',
  'destroySyncClient',
  'syncGetWsUrl',
  'syncSetSessionCode',
  'syncOnConnected',
  'syncOnBinaryMessage',
  'syncOnBinaryMessages',
  'syncOnTextMessage',
  'syncOnTextMessages',
  'syncOnDisconnected',
  'syncOnSnapshotImported',
  'syncQueueLocalUpdate',
  'syncDrain',
  'syncFocusFiles',
  'syncUnfocusFiles',
  'syncBodyFiles',
]);

export function createSyncOverlay(
  originalBackend: Backend,
  syncAdapter: ExtismSyncBackendAdapter,
): Backend {
  return new Proxy(originalBackend, {
    get(target, prop, receiver) {
      if (typeof prop === 'string' && SYNC_METHOD_NAMES.has(prop)) {
        const method = (syncAdapter as any)[prop];
        if (typeof method === 'function') {
          return method.bind(syncAdapter);
        }
      }
      return Reflect.get(target, prop, receiver);
    },
  });
}
