/**
 * HttpBackend — Backend implementation that talks to a local REST server.
 *
 * Used by `diaryx edit` which starts a lightweight axum server exposing
 * the diaryx_core Command/Response API over HTTP, bypassing the CRDT sync
 * layer entirely.
 *
 * URL params detected by the factory:
 *   ?backend=http&api_url=http://localhost:PORT
 */

import type {
  Backend,
  BackendEventType,
  BackendEventListener,
  Config,
  ImportResult,
  PluginInspection,
} from "./interface";
import type { Command, Response } from "./generated";
import { BackendError, setNativePluginBackend } from "./interface";

export class HttpBackend implements Backend {
  private apiUrl: string;
  private _ready = false;
  private workspacePath = "workspace";
  private _nativePlugins = false;
  private listeners = new Map<BackendEventType, Set<BackendEventListener>>();

  constructor(apiUrl: string) {
    // Strip trailing slash
    this.apiUrl = apiUrl.replace(/\/+$/, "");
  }

  async init(): Promise<void> {
    // Fetch workspace info from the server
    try {
      const res = await fetch(`${this.apiUrl}/api/workspace`);
      if (res.ok) {
        const info = await res.json();
        this.workspacePath = info.workspace_path ?? "workspace";
        this._nativePlugins = info.native_plugins === true;
        // Set the flag immediately so isNativePluginBackend() works before
        // the singleton is stored in globalThis by the backend factory.
        setNativePluginBackend(this._nativePlugins);
      }
    } catch (e) {
      console.warn("[HttpBackend] Could not fetch workspace info:", e);
    }
    this._ready = true;
    console.log("[HttpBackend] Initialized, api:", this.apiUrl, "nativePlugins:", this._nativePlugins);
  }

  /** Whether the server supports native plugin loading (plugins feature). */
  get nativePlugins(): boolean {
    return this._nativePlugins;
  }

  isReady(): boolean {
    return this._ready;
  }

  getWorkspacePath(): string {
    return this.workspacePath;
  }

  getConfig(): Config | null {
    return null;
  }

  getAppPaths(): Record<string, string | boolean | null> | null {
    return null;
  }

  async execute(command: Command): Promise<Response> {
    const res = await fetch(`${this.apiUrl}/api/execute`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(command),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new BackendError(text, "HttpError", undefined);
    }

    return res.json();
  }

  on(event: BackendEventType, listener: BackendEventListener): void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(listener);
  }

  off(event: BackendEventType, listener: BackendEventListener): void {
    this.listeners.get(event)?.delete(listener);
  }

  async persist(): Promise<void> {
    // No-op — the server writes directly to disk
  }

  async readBinary(path: string): Promise<Uint8Array> {
    const res = await fetch(`${this.apiUrl}/api/binary/${encodeURI(path)}`);
    if (!res.ok) {
      throw new BackendError(
        `Failed to read binary: ${res.statusText}`,
        "HttpError",
        path,
      );
    }
    const buf = await res.arrayBuffer();
    return new Uint8Array(buf);
  }

  async writeBinary(path: string, data: Uint8Array): Promise<void> {
    const res = await fetch(`${this.apiUrl}/api/binary/${encodeURI(path)}`, {
      method: "POST",
      headers: { "Content-Type": "application/octet-stream" },
      body: new Uint8Array(data) as Uint8Array<ArrayBuffer>,
    });
    if (!res.ok) {
      throw new BackendError(
        `Failed to write binary: ${res.statusText}`,
        "HttpError",
        path,
      );
    }
  }

  async importFromZip(): Promise<ImportResult> {
    return { success: false, files_imported: 0, error: "Import not supported in HTTP backend" };
  }

  // ===========================================================================
  // Plugin management (native — mirrors Tauri backend)
  // ===========================================================================

  async installPlugin(wasmBytes: Uint8Array): Promise<string> {
    const res = await fetch(`${this.apiUrl}/api/plugins/install`, {
      method: "POST",
      headers: { "Content-Type": "application/wasm" },
      body: new Uint8Array(wasmBytes) as Uint8Array<ArrayBuffer>,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new BackendError(text, "HttpError", undefined);
    }
    return res.text();
  }

  async uninstallPlugin(pluginId: string): Promise<void> {
    const res = await fetch(
      `${this.apiUrl}/api/plugins/${encodeURIComponent(pluginId)}`,
      { method: "DELETE" },
    );
    if (!res.ok) {
      const text = await res.text();
      throw new BackendError(text, "HttpError", undefined);
    }
  }

  async inspectPlugin(wasmBytes: Uint8Array): Promise<PluginInspection> {
    const res = await fetch(`${this.apiUrl}/api/plugins/inspect`, {
      method: "POST",
      headers: { "Content-Type": "application/wasm" },
      body: new Uint8Array(wasmBytes) as Uint8Array<ArrayBuffer>,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new BackendError(text, "HttpError", undefined);
    }
    const data = await res.json();
    return {
      pluginId: data.plugin_id,
      pluginName: data.plugin_name,
      requestedPermissions: data.requested_permissions,
    };
  }

  async getPluginComponentHtml(
    pluginId: string,
    componentId: string,
  ): Promise<string> {
    const res = await fetch(
      `${this.apiUrl}/api/plugins/${encodeURIComponent(pluginId)}/component/${encodeURIComponent(componentId)}`,
    );
    if (!res.ok) {
      const text = await res.text();
      throw new BackendError(text, "HttpError", undefined);
    }
    return res.text();
  }
}
