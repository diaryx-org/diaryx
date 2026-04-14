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
} from "./interface";
import type { Command, Response } from "./generated";
import { BackendError } from "./interface";

export class HttpBackend implements Backend {
  private apiUrl: string;
  private _ready = false;
  private workspacePath = "workspace";
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
      }
    } catch (e) {
      console.warn("[HttpBackend] Could not fetch workspace info:", e);
    }
    this._ready = true;
    console.log("[HttpBackend] Initialized, api:", this.apiUrl);
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
}
