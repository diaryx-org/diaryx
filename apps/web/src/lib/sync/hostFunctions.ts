/**
 * Host function implementations for the Extism sync plugin.
 *
 * These functions run on the host (browser) side and are called by the
 * diaryx_sync_extism guest WASM plugin via Extism host function imports.
 * They provide filesystem access, CRDT storage (IndexedDB via SQLite),
 * event dispatch, and logging.
 */

import type { CallContext } from '@extism/extism';
import type { Backend } from '../backend/interface';
import {
  getSqliteStorageSync,
} from '../storage/sqliteStorage';

// ============================================================================
// Types
// ============================================================================

/**
 * Context passed to host functions via Extism's hostContext mechanism.
 */
export interface SyncHostContext {
  /** Backend for filesystem operations. */
  backend: Backend;
  /** Callback for sync events emitted by the guest plugin. */
  onSyncEvent?: (eventJson: string) => void;
}

// ============================================================================
// Host Function Implementations
// ============================================================================

/**
 * Build the host functions map for Extism plugin creation.
 *
 * These are registered under the `"extism:host/user"` namespace, matching
 * the Extism PDK's `#[host_fn]` convention.
 *
 * @param hostCtx - Shared context providing backend + event dispatch
 * @returns Host functions map compatible with `ExtismPluginOptions.functions`
 */
export function buildHostFunctions(hostCtx: SyncHostContext): {
  [namespace: string]: {
    [func: string]: (callContext: CallContext, ...args: any[]) => any;
  };
} {
  let loggedWsRequestStub = false;
  const emptyResult = (callContext: CallContext) => callContext.store('');
  const readInput = (callContext: CallContext, addr: unknown) => {
    if (typeof addr !== 'bigint' && typeof addr !== 'number') return null;
    return callContext.read(addr);
  };
  const parseJsonInput = <T>(callContext: CallContext, addr: unknown): T | null => {
    const input = readInput(callContext, addr);
    if (!input) return null;
    try {
      return JSON.parse(input.text()) as T;
    } catch {
      return null;
    }
  };

  return {
    'extism:host/user': {
      host_log(callContext: CallContext, addr: bigint) {
        const input = readInput(callContext, addr);
        if (!input) return emptyResult(callContext);
        const { level, message } = JSON.parse(input.text());
        const prefix = '[SyncPlugin/WASM]';
        switch (level) {
          case 'error':
            console.error(prefix, message);
            break;
          case 'warn':
            console.warn(prefix, message);
            break;
          case 'debug':
            console.debug(prefix, message);
            break;
          case 'trace':
            // skip trace in browser
            break;
          default:
            console.log(prefix, message);
        }
        return emptyResult(callContext);
      },

      async host_read_file(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{ path?: string }>(callContext, addr);
        const path = typeof input?.path === 'string' ? input.path : '';
        if (!path) return callContext.store('');

        try {
          const response = await hostCtx.backend.execute({
            type: 'ReadFile',
            params: { path },
          } as any);
          if (response.type === 'String') {
            return callContext.store(response.data);
          }
        } catch {
          // Read failures are expected for transient temp files (e.g. *.tmp).
        }
        return callContext.store('');
      },

      async host_list_files(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{ prefix?: string }>(callContext, addr);
        const prefix = typeof input?.prefix === 'string' ? input.prefix : '';

        try {
          const response = await hostCtx.backend.execute({
            type: 'GetFilesystemTree',
            params: {
              path: prefix.length > 0 ? prefix : '.',
              show_hidden: true,
              depth: null,
            },
          } as any);

          if (response.type !== 'Tree') {
            return callContext.store(JSON.stringify([]));
          }

          const files: string[] = [];
          const walk = (node: { path?: string; children?: Array<any> } | null | undefined) => {
            if (!node || typeof node.path !== 'string') return;
            const children = Array.isArray(node.children) ? node.children : [];
            if (children.length === 0) {
              files.push(node.path);
              return;
            }
            for (const child of children) walk(child);
          };
          // Skip the root directory node — only walk its children
          const root = response.data as { children?: Array<any> } | null;
          if (root && Array.isArray(root.children)) {
            for (const child of root.children) walk(child);
          }

          return callContext.store(JSON.stringify(files));
        } catch {
          return callContext.store(JSON.stringify([]));
        }
      },

      async host_file_exists(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{ path?: string }>(callContext, addr);
        const path = typeof input?.path === 'string' ? input.path : '';
        if (!path) return callContext.store('false');

        try {
          const response = await hostCtx.backend.execute({
            type: 'FileExists',
            params: { path },
          } as any);
          if (response.type === 'Bool') {
            return callContext.store(response.data ? 'true' : 'false');
          }
        } catch {
          // fall through
        }
        return callContext.store('false');
      },

      async host_write_file(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{ path?: string; content?: string }>(callContext, addr);
        const path = typeof input?.path === 'string' ? input.path : '';
        const content = typeof input?.content === 'string' ? input.content : '';
        if (!path) return emptyResult(callContext);

        try {
          await hostCtx.backend.execute({
            type: 'WriteFile',
            params: { path, content },
          } as any);
        } catch (e: unknown) {
          console.error('[SyncPlugin] host_write_file failed:', path, e);
        }
        return emptyResult(callContext);
      },

      async host_write_binary(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{ path?: string; content?: string }>(callContext, addr);
        const path = typeof input?.path === 'string' ? input.path : '';
        const b64content = typeof input?.content === 'string' ? input.content : '';
        if (!path) return emptyResult(callContext);

        try {
          await hostCtx.backend.writeBinary(path, base64ToUint8Array(b64content));
        } catch (e: unknown) {
          console.error('[SyncPlugin] host_write_binary failed:', path, e);
        }
        return emptyResult(callContext);
      },

      host_emit_event(callContext: CallContext, addr: bigint) {
        const eventJson = readInput(callContext, addr)?.text() ?? '';
        hostCtx.onSyncEvent?.(eventJson);
        return emptyResult(callContext);
      },

      host_storage_get(callContext: CallContext, addr: bigint) {
        const input = readInput(callContext, addr);
        if (!input) return callContext.store('');
        const { key } = JSON.parse(input.text());
        const storage = getSqliteStorageSync();
        if (!storage) {
          return callContext.store('');
        }
        const data = storage.loadDoc(key);
        if (!data) {
          return callContext.store('');
        }
        // Encode as base64 for transport through JSON/text boundary
        const b64 = uint8ArrayToBase64(data);
        return callContext.store(b64);
      },

      host_storage_set(callContext: CallContext, addr: bigint) {
        const input = readInput(callContext, addr);
        if (!input) return emptyResult(callContext);
        const { key, data: b64data } = JSON.parse(input.text());
        const storage = getSqliteStorageSync();
        if (!storage) {
          console.error('[SyncPlugin] host_storage_set: storage not initialized');
          return emptyResult(callContext);
        }
        const bytes = base64ToUint8Array(b64data);
        // Use empty state vector — the guest manages its own state vectors
        storage.saveDoc(key, bytes, new Uint8Array(0));
        return emptyResult(callContext);
      },

      host_get_timestamp(callContext: CallContext, _addr: bigint) {
        const now = Date.now();
        return callContext.store(now.toString());
      },

      async host_http_request(callContext: CallContext, addr: bigint) {
        const input = parseJsonInput<{
          url?: string;
          method?: string;
          headers?: Record<string, string>;
          body?: string;
        }>(callContext, addr);
        const url = input?.url ?? '';
        const method = input?.method ?? 'GET';
        if (!url) {
          return callContext.store(JSON.stringify({ status: 0, headers: {}, body: 'no url' }));
        }
        try {
          const resp = await fetch(url, {
            method,
            headers: input?.headers ?? {},
            body: input?.body ?? undefined,
          });
          const respHeaders: Record<string, string> = {};
          resp.headers.forEach((v, k) => { respHeaders[k] = v; });
          const body = await resp.text();
          return callContext.store(JSON.stringify({ status: resp.status, headers: respHeaders, body }));
        } catch (e: unknown) {
          const msg = e instanceof Error ? e.message : String(e);
          return callContext.store(JSON.stringify({ status: 0, headers: {}, body: msg }));
        }
      },

      host_ws_request(callContext: CallContext, addr: bigint) {
        // Forward-compatible stub for plugin-initiated WS ownership. The
        // current sync path keeps WebSocket lifecycle in host transport.
        const input = readInput(callContext, addr);
        if (!loggedWsRequestStub) {
          loggedWsRequestStub = true;
          console.warn(
            '[SyncPlugin] host_ws_request is not enabled; host transport owns WebSocket lifecycle.',
            input ? input.text() : '',
          );
        }
        return emptyResult(callContext);
      },
    },
  };
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

function base64ToUint8Array(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}
