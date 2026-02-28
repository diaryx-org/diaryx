/**
 * Extism Browser Loader — loads WASM plugins in the browser via the Extism JS SDK.
 *
 * The same .wasm plugin files that run natively via the Rust `extism` crate
 * can be loaded here using `@extism/extism`. Guest plugins communicate through
 * the same JSON protocol defined in `diaryx_extism::protocol`.
 */

import createPlugin, { type Plugin as ExtismPlugin, type CallContext } from '@extism/extism';
import type {
  PluginManifest,
  PluginCapability,
  UiContribution,
} from '$lib/backend/generated';
import { getBackendSync } from '$lib/backend';

// ============================================================================
// Protocol types (mirrors diaryx_extism::protocol)
// ============================================================================

export interface GuestManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  ui: UiContribution[];
  commands: string[];
}

export interface GuestEvent {
  event_type: string;
  payload: unknown;
}

export interface CommandResponse {
  success: boolean;
  data?: unknown;
  error?: string;
}

// ============================================================================
// Browser plugin wrapper
// ============================================================================

export interface BrowserExtismPlugin {
  /** The plugin's manifest, converted to the core PluginManifest format. */
  manifest: PluginManifest;
  /** Send a lifecycle event to the guest. */
  callEvent(event: GuestEvent): Promise<void>;
  /** Dispatch a command to the guest. */
  callCommand(cmd: string, params: unknown): Promise<CommandResponse>;
  /** Get the guest's current configuration. */
  getConfig(): Promise<Record<string, unknown>>;
  /** Update the guest's configuration. */
  setConfig(config: Record<string, unknown>): Promise<void>;
  /** Release the plugin's resources. */
  close(): Promise<void>;
}

// ============================================================================
// Host function definitions
// ============================================================================

function buildHostFunctions() {
  return {
    'extism:host/user': {
      host_log(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { level: string; message: string }
            | undefined;
          if (!input) return cp.store('');
          const prefix = `[extism-plugin]`;
          switch (input.level) {
            case 'error':
              console.error(prefix, input.message);
              break;
            case 'warn':
              console.warn(prefix, input.message);
              break;
            case 'debug':
              console.debug(prefix, input.message);
              break;
            default:
              console.log(prefix, input.message);
          }
          return cp.store('');
        } catch {
          return cp.store('');
        }
      },
      async host_read_file(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { path: string }
            | undefined;
          if (!input) return cp.store('');
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: 'ReadFile',
            params: { path: input.path },
          } as any);
          if (response.type === 'String') {
            return cp.store(response.data);
          }
          return cp.store('');
        } catch {
          return cp.store('');
        }
      },
      async host_list_files(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { prefix: string }
            | undefined;
          if (!input) return cp.store('[]');
          const prefix = typeof input.prefix === 'string' ? input.prefix : '';
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: 'GetFilesystemTree',
            params: {
              path: prefix.length > 0 ? prefix : '.',
              show_hidden: true,
              depth: null,
            },
          } as any);
          if (response.type !== 'Tree') {
            return cp.store('[]');
          }
          // Walk tree to extract leaf file paths (skip the root directory node)
          const files: string[] = [];
          const walk = (node: any) => {
            if (!node || typeof node.path !== 'string') return;
            const children = Array.isArray(node.children) ? node.children : [];
            if (children.length === 0) {
              files.push(node.path);
              return;
            }
            for (const child of children) walk(child);
          };
          const root = response.data;
          if (root && Array.isArray(root.children)) {
            for (const child of root.children) walk(child);
          }
          return cp.store(JSON.stringify(files));
        } catch {
          return cp.store('[]');
        }
      },
      async host_file_exists(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { path: string }
            | undefined;
          if (!input) return cp.store('false');
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: 'FileExists',
            params: { path: input.path },
          } as any);
          if (response.type === 'Bool') {
            return cp.store(response.data ? 'true' : 'false');
          }
          return cp.store('false');
        } catch {
          return cp.store('false');
        }
      },
      host_storage_get(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string } | undefined;
          if (!input?.key) return cp.store('');
          const raw = localStorage.getItem(`diaryx-plugin:${input.key}`);
          if (!raw) return cp.store('');
          // Return in the same {data: base64} format the Rust host uses
          return cp.store(raw);
        } catch {
          return cp.store('');
        }
      },
      host_storage_set(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string; data: string } | undefined;
          if (!input?.key) return cp.store('');
          // Store the base64 data wrapped in JSON matching Rust host format
          localStorage.setItem(`diaryx-plugin:${input.key}`, JSON.stringify({ data: input.data }));
          return cp.store('');
        } catch {
          return cp.store('');
        }
      },
      host_get_timestamp(cp: CallContext, _offs: bigint) {
        return cp.store(Date.now().toString());
      },
      async host_http_request(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { url: string; method: string; headers: Record<string, string>; body?: string }
            | undefined;
          if (!input) return cp.store(JSON.stringify({ status: 0, headers: {}, body: 'no input' }));
          const resp = await fetch(input.url, {
            method: input.method,
            headers: input.headers,
            body: input.body ?? undefined,
          });
          const respHeaders: Record<string, string> = {};
          resp.headers.forEach((v, k) => { respHeaders[k] = v; });
          const body = await resp.text();
          return cp.store(JSON.stringify({ status: resp.status, headers: respHeaders, body }));
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          return cp.store(JSON.stringify({ status: 0, headers: {}, body: msg }));
        }
      },
    },
  };
}

// ============================================================================
// Manifest conversion
// ============================================================================

function convertGuestManifest(guest: GuestManifest): PluginManifest {
  const capabilities: PluginCapability[] = guest.capabilities
    .map((cap): PluginCapability | null => {
      switch (cap) {
        case 'file_events':
          return 'FileEvents' as PluginCapability;
        case 'workspace_events':
          return 'WorkspaceEvents' as PluginCapability;
        case 'custom_commands':
          return {
            CustomCommands: { commands: guest.commands },
          } as unknown as PluginCapability;
        default:
          console.warn(`[extism] Unknown capability: ${cap}`);
          return null;
      }
    })
    .filter((c): c is PluginCapability => c !== null);

  return {
    id: guest.id,
    name: guest.name,
    version: guest.version,
    description: guest.description,
    capabilities,
    ui: guest.ui ?? [],
  };
}

// ============================================================================
// Plugin loader
// ============================================================================

/**
 * Load a WASM plugin from raw bytes in the browser.
 *
 * Creates an Extism plugin instance with WASI support and host functions
 * for filesystem access (routed through the backend worker).
 */
export async function loadBrowserPlugin(
  wasmBytes: ArrayBuffer,
): Promise<BrowserExtismPlugin> {
  // Extism v2 needs JSPI (WebAssembly.Suspending/promising) for async host
  // functions when running on the main thread. WebKit doesn't support JSPI
  // yet (only Safari Technology Preview 238+). The Extism worker fallback
  // (runInWorker) needs cross-origin isolation (COOP+COEP headers) which
  // breaks WebKit's worker module loading. So on WebKit without JSPI,
  // plugins are unavailable until Safari ships JSPI.
  const hasJSPI =
    typeof (WebAssembly as any).Suspending === 'function' &&
    typeof (WebAssembly as any).promising === 'function';

  if (!hasJSPI) {
    throw new Error(
      'Plugins require WebAssembly JSPI (Suspending/promising), which is not available in this browser. ' +
        'Use Chrome, Firefox 139+, or Safari Technology Preview 238+.',
    );
  }

  const plugin: ExtismPlugin = await createPlugin(wasmBytes, {
    useWasi: true,
    functions: buildHostFunctions(),
  });

  // Call guest `manifest` export to get the plugin's manifest.
  const manifestOutput = await plugin.call('manifest', '');
  if (!manifestOutput) {
    await plugin.close();
    throw new Error('Plugin manifest() returned null');
  }
  const guestManifest: GuestManifest = manifestOutput.json();
  const manifest = convertGuestManifest(guestManifest);

  // Serialize all calls to the WASM plugin. WASM modules are single-threaded;
  // concurrent plugin.call() invocations cause response mix-ups.
  let callQueue: Promise<unknown> = Promise.resolve();
  function enqueue<T>(fn: () => Promise<T>): Promise<T> {
    const next = callQueue.then(fn, fn);
    callQueue = next.then(() => {}, () => {});
    return next;
  }

  return {
    manifest,

    async callEvent(event: GuestEvent): Promise<void> {
      return enqueue(async () => {
        try {
          await plugin.call('on_event', JSON.stringify(event));
        } catch (e) {
          console.warn(`[extism] ${manifest.id}: on_event failed:`, e);
        }
      });
    },

    async callCommand(
      cmd: string,
      params: unknown,
    ): Promise<CommandResponse> {
      return enqueue(async () => {
        const request = JSON.stringify({ command: cmd, params });
        try {
          const output = await plugin.call('handle_command', request);
          if (!output) return { success: false, error: 'No response from plugin' };
          return output.json() as CommandResponse;
        } catch (e) {
          return {
            success: false,
            error: e instanceof Error ? e.message : String(e),
          };
        }
      });
    },

    async getConfig(): Promise<Record<string, unknown>> {
      return enqueue(async () => {
        try {
          const output = await plugin.call('get_config', '');
          if (!output) return {};
          const text = output.text();
          if (!text) return {};
          return JSON.parse(text);
        } catch {
          return {};
        }
      });
    },

    async setConfig(config: Record<string, unknown>): Promise<void> {
      return enqueue(async () => {
        try {
          await plugin.call('set_config', JSON.stringify(config));
        } catch (e) {
          console.warn(`[extism] ${manifest.id}: set_config failed:`, e);
        }
      });
    },

    async close(): Promise<void> {
      await plugin.close();
    },
  };
}
