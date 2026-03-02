/**
 * Extism Browser Loader — loads WASM plugins in the browser via the Extism JS SDK.
 *
 * The same .wasm plugin files that run natively via the Rust `extism` crate
 * can be loaded here using `@extism/extism`. Guest plugins communicate through
 * the same JSON protocol defined in `diaryx_extism::protocol`.
 */

import createPlugin, {
  type Plugin as ExtismPlugin,
  type CallContext,
} from "@extism/extism";
import { handleHostRunWasiModule, type WasiRunRequest } from "./wasiRunner";
import type {
  PluginManifest,
  PluginCapability,
  UiContribution,
} from "$lib/backend/generated";
import { getBackendSync } from "$lib/backend";

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
  cli?: unknown[];
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

export interface BrowserPluginRuntimeSupport {
  supported: boolean;
  reason?: string;
  useWorkerFallback?: boolean;
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
  /**
   * Execute a typed Command (same format as backend.execute).
   * Calls the guest's `execute_typed_command` export.
   * Returns the Response if handled, null if the plugin doesn't handle this command.
   */
  callTypedCommand(command: unknown): Promise<unknown | null>;
  /** Call a named export with binary input, returning binary output. */
  callBinary(exportName: string, data: Uint8Array): Promise<Uint8Array | null>;
  /** Get the guest's current configuration. */
  getConfig(): Promise<Record<string, unknown>>;
  /** Update the guest's configuration. */
  setConfig(config: Record<string, unknown>): Promise<void>;
  /** Call a named render export on the plugin (for EditorExtension rendering). */
  callRender(
    exportName: string,
    source: string,
    options: Record<string, unknown>,
  ): Promise<{ html?: string; error?: string }>;
  /** Release the plugin's resources. */
  close(): Promise<void>;
}

// ============================================================================
// Host function definitions
// ============================================================================

function buildHostFunctions() {
  return {
    "extism:host/user": {
      host_log(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { level: string; message: string }
            | undefined;
          if (!input) return cp.store("");
          const prefix = `[extism-plugin]`;
          switch (input.level) {
            case "error":
              console.error(prefix, input.message);
              break;
            case "warn":
              console.warn(prefix, input.message);
              break;
            case "debug":
              console.debug(prefix, input.message);
              break;
            default:
              console.log(prefix, input.message);
          }
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_read_file(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { path: string } | undefined;
          if (!input) return cp.store("");
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "ReadFile",
            params: { path: input.path },
          } as any);
          if (response.type === "String") {
            return cp.store(response.data);
          }
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_list_files(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { prefix: string } | undefined;
          if (!input) return cp.store("[]");
          const prefix = typeof input.prefix === "string" ? input.prefix : "";
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "GetFilesystemTree",
            params: {
              path: prefix.length > 0 ? prefix : ".",
              show_hidden: true,
              depth: null,
            },
          } as any);
          if (response.type !== "Tree") {
            return cp.store("[]");
          }
          // Walk tree to extract leaf file paths (skip the root directory node)
          const files: string[] = [];
          const walk = (node: any) => {
            if (!node || typeof node.path !== "string") return;
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
          return cp.store("[]");
        }
      },
      async host_file_exists(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { path: string } | undefined;
          if (!input) return cp.store("false");
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "FileExists",
            params: { path: input.path },
          } as any);
          if (response.type === "Bool") {
            return cp.store(response.data ? "true" : "false");
          }
          return cp.store("false");
        } catch {
          return cp.store("false");
        }
      },
      host_storage_get(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string } | undefined;
          if (!input?.key) return cp.store("");
          const raw = localStorage.getItem(`diaryx-plugin:${input.key}`);
          if (!raw) return cp.store("");
          // Return in the same {data: base64} format the Rust host uses
          return cp.store(raw);
        } catch {
          return cp.store("");
        }
      },
      host_storage_set(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { key: string; data: string }
            | undefined;
          if (!input?.key) return cp.store("");
          // Store the base64 data wrapped in JSON matching Rust host format
          localStorage.setItem(
            `diaryx-plugin:${input.key}`,
            JSON.stringify({ data: input.data }),
          );
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      host_get_timestamp(cp: CallContext, _offs: bigint) {
        return cp.store(Date.now().toString());
      },
      async host_http_request(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | {
                url: string;
                method: string;
                headers: Record<string, string>;
                body?: string;
                body_base64?: string;
              }
            | undefined;
          if (!input)
            return cp.store(
              JSON.stringify({ status: 0, headers: {}, body: "no input" }),
            );
          let fetchBody: BodyInit | undefined;
          if (input.body_base64) {
            const binary = atob(input.body_base64);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) {
              bytes[i] = binary.charCodeAt(i);
            }
            fetchBody = bytes;
          } else {
            fetchBody = input.body ?? undefined;
          }
          const resp = await fetch(input.url, {
            method: input.method,
            headers: input.headers,
            body: fetchBody,
          });
          const respHeaders: Record<string, string> = {};
          resp.headers.forEach((v, k) => {
            respHeaders[k] = v;
          });
          const bytes = new Uint8Array(await resp.arrayBuffer());
          let body = "";
          try {
            body = new TextDecoder().decode(bytes);
          } catch {
            body = "";
          }
          let binary = "";
          for (let i = 0; i < bytes.length; i++) {
            binary += String.fromCharCode(bytes[i]);
          }
          const body_base64 = btoa(binary);
          return cp.store(
            JSON.stringify({
              status: resp.status,
              headers: respHeaders,
              body,
              body_base64,
            }),
          );
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          return cp.store(
            JSON.stringify({ status: 0, headers: {}, body: msg }),
          );
        }
      },
      async host_write_file(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { path: string; content: string }
            | undefined;
          if (!input) return cp.store("");
          const backend = getBackendSync();
          await backend.execute({
            type: "WriteFile",
            params: { path: input.path, content: input.content },
          } as any);
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_write_binary(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { path: string; content: string }
            | undefined;
          if (!input) return cp.store("");
          // Decode base64 to Uint8Array
          const binary = atob(input.content);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) {
            bytes[i] = binary.charCodeAt(i);
          }
          const backend = getBackendSync();
          await backend.writeBinary(input.path, bytes);
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      host_emit_event(cp: CallContext, offs: bigint) {
        try {
          const eventJson = cp.read(offs)?.text();
          if (!eventJson) return cp.store("");
          const backend = getBackendSync();
          if (!backend) return cp.store("");
          const event = JSON.parse(eventJson);
          backend.emitFileSystemEvent?.(event);
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      host_ws_request(cp: CallContext, _offs: bigint) {
        // No-op stub — WebSocket lifecycle is managed by the TypeScript transport.
        return cp.store("");
      },
      async host_run_wasi_module(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as WasiRunRequest | undefined;
          if (!input) {
            return cp.store(
              JSON.stringify({
                exit_code: -1,
                stdout: "",
                stderr: "host_run_wasi_module: no input",
              }),
            );
          }
          const result = await handleHostRunWasiModule(input);
          return cp.store(JSON.stringify(result));
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          return cp.store(
            JSON.stringify({
              exit_code: -1,
              stdout: "",
              stderr: `host_run_wasi_module: ${msg}`,
            }),
          );
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
        case "file_events":
          return "FileEvents" as PluginCapability;
        case "workspace_events":
          return "WorkspaceEvents" as PluginCapability;
        case "custom_commands":
          return {
            CustomCommands: { commands: guest.commands },
          } as unknown as PluginCapability;
        case "editor_extension":
          return "EditorExtension" as PluginCapability;
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
    cli: [],
  };
}

// ============================================================================
// Plugin loader
// ============================================================================

function isWebKitBrowser(): boolean {
  if (typeof navigator === "undefined") return false;
  const ua = navigator.userAgent;
  const isWebKitEngine = /AppleWebKit/i.test(ua);
  const isChromiumFamily = /Chrome|CriOS|Edg|OPR|SamsungBrowser/i.test(ua);
  return isWebKitEngine && !isChromiumFamily;
}

function hasJSPI(): boolean {
  return (
    typeof (WebAssembly as any).Suspending === "function" &&
    typeof (WebAssembly as any).promising === "function"
  );
}

function canUseWorkerFallback(): boolean {
  return typeof window !== "undefined" && window.crossOriginIsolated === true;
}

/**
 * Reports whether browser-loaded Extism plugins are supported in this runtime.
 */
export function getBrowserPluginRuntimeSupport(): BrowserPluginRuntimeSupport {
  if (hasJSPI()) {
    return { supported: true, useWorkerFallback: false };
  }

  if (canUseWorkerFallback()) {
    return {
      supported: true,
      useWorkerFallback: true,
      reason:
        "WebAssembly JSPI is unavailable; using Extism worker fallback because cross-origin isolation is enabled.",
    };
  }

  if (isWebKitBrowser()) {
    return {
      supported: false,
      reason:
        "Plugins are unavailable because WebAssembly JSPI is missing and worker fallback requires cross-origin isolation. " +
        "Use a cross-origin-isolated origin (COOP+COEP), Chrome, Firefox 139+, or Safari Technology Preview 238+.",
    };
  }

  return {
    supported: false,
    reason:
      "Plugins require WebAssembly JSPI (Suspending/promising), or cross-origin isolation for worker fallback. " +
      "Enable COOP+COEP or use Chrome, Firefox 139+, or Safari Technology Preview 238+.",
  };
}

/**
 * Load a WASM plugin from raw bytes in the browser.
 *
 * Creates an Extism plugin instance with WASI support and host functions
 * for filesystem access (routed through the backend worker).
 */
export async function loadBrowserPlugin(
  wasmBytes: ArrayBuffer,
): Promise<BrowserExtismPlugin> {
  const support = getBrowserPluginRuntimeSupport();
  if (!support.supported) {
    throw new Error(
      support.reason ?? "Browser plugins are not supported in this runtime.",
    );
  }

  const plugin: ExtismPlugin = await createPlugin(wasmBytes, {
    useWasi: true,
    runInWorker: support.useWorkerFallback ?? false,
    functions: buildHostFunctions(),
  });

  // Call guest `manifest` export to get the plugin's manifest.
  const manifestOutput = await plugin.call("manifest", "");
  if (!manifestOutput) {
    await plugin.close();
    throw new Error("Plugin manifest() returned null");
  }
  const guestManifest: GuestManifest = manifestOutput.json();
  const manifest = convertGuestManifest(guestManifest);

  // Serialize all calls to the WASM plugin. WASM modules are single-threaded;
  // concurrent plugin.call() invocations cause response mix-ups.
  let callQueue: Promise<unknown> = Promise.resolve();
  function enqueue<T>(fn: () => Promise<T>): Promise<T> {
    const next = callQueue.then(fn, fn);
    callQueue = next.then(
      () => {},
      () => {},
    );
    return next;
  }

  return {
    manifest,

    async callEvent(event: GuestEvent): Promise<void> {
      return enqueue(async () => {
        try {
          await plugin.call("on_event", JSON.stringify(event));
        } catch (e) {
          console.warn(`[extism] ${manifest.id}: on_event failed:`, e);
        }
      });
    },

    async callCommand(cmd: string, params: unknown): Promise<CommandResponse> {
      return enqueue(async () => {
        const request = JSON.stringify({ command: cmd, params });
        try {
          const output = await plugin.call("handle_command", request);
          if (!output)
            return { success: false, error: "No response from plugin" };
          return output.json() as CommandResponse;
        } catch (e) {
          return {
            success: false,
            error: e instanceof Error ? e.message : String(e),
          };
        }
      });
    },

    async callTypedCommand(command: unknown): Promise<unknown | null> {
      return enqueue(async () => {
        try {
          const output = await plugin.call(
            "execute_typed_command",
            JSON.stringify(command),
          );
          if (!output) return null;
          const text = output.text();
          if (!text) return null;
          return JSON.parse(text);
        } catch (e) {
          console.error(
            `[extism] ${manifest.id}: execute_typed_command failed:`,
            e,
          );
          return null;
        }
      });
    },

    async callBinary(
      exportName: string,
      data: Uint8Array,
    ): Promise<Uint8Array | null> {
      return enqueue(async () => {
        try {
          const output = await plugin.call(exportName, data);
          if (!output) return null;
          return output.bytes();
        } catch (e) {
          console.error(
            `[extism] ${manifest.id}: ${exportName} (binary) failed:`,
            e,
          );
          return null;
        }
      });
    },

    async callRender(
      exportName: string,
      source: string,
      options: Record<string, unknown>,
    ): Promise<{ html?: string; error?: string }> {
      return enqueue(async () => {
        try {
          const input = JSON.stringify({ source, ...options });
          const output = await plugin.call(exportName, input);
          if (!output) return { error: "No response from plugin render" };
          return output.json() as { html?: string; error?: string };
        } catch (e) {
          return { error: e instanceof Error ? e.message : String(e) };
        }
      });
    },

    async getConfig(): Promise<Record<string, unknown>> {
      return enqueue(async () => {
        try {
          const output = await plugin.call("get_config", "");
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
          await plugin.call("set_config", JSON.stringify(config));
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
