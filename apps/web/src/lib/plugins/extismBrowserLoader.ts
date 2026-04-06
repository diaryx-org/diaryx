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
import { dispatchCommand } from "$lib/plugins/browserPluginManager.svelte";
import {
  deletePluginSecret,
  getPluginSecret,
  setPluginSecret,
} from "./pluginSecretStore";
import type {
  PluginManifest,
  PluginCapability,
  UiContribution,
} from "$lib/backend/generated";
import { getBackendSync } from "$lib/backend";
import type { FileSystemEvent } from "$lib/backend/interface";
import {
  permissionStore,
  type PermissionType,
  type PluginPermissions,
  type PluginConfig,
} from "@/models/stores/permissionStore.svelte";
import {
  getPluginStoragePath,
} from "$lib/workspace/workspaceAssetStorage";
import { collectFilesystemTreePaths } from "./filesystemTreePaths";
import { normalizeExtismHostPath } from "./extismHostPaths";
import { proxyFetch } from "$lib/backend/proxyFetch";

// ============================================================================
// Helpers
// ============================================================================

/** Race a promise against a timeout. Rejects with `message` if the timeout fires first. */
function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    promise.then(
      (v) => { clearTimeout(timer); resolve(v); },
      (e) => { clearTimeout(timer); reject(e); },
    );
  });
}

// ============================================================================
// Protocol types (mirrors diaryx_extism::protocol)
// ============================================================================

export interface GuestManifest {
  protocol_version?: number;
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  ui: UiContribution[];
  commands: string[];
  cli?: unknown[];
  requested_permissions?: RequestedPermissionsManifest;
  conversions?: string[];
}

export interface RequestedPermissionsManifest {
  defaults: PluginPermissions;
  reasons?: Partial<Record<PermissionType, string>>;
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

export interface BrowserPluginCallOptions {
  /** Provider of temporary user-selected files for this command call only. */
  getFile?: (key: string) => Promise<Uint8Array | null>;
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
  /** Plugin-declared default permissions and reasons (optional). */
  requestedPermissions?: RequestedPermissionsManifest;
  /** Call an optional lifecycle export (`init`/`shutdown`) with JSON input. */
  callLifecycle(
    exportName: "init" | "shutdown",
    payload?: unknown,
  ): Promise<void>;
  /** Send a lifecycle event to the guest. */
  callEvent(event: GuestEvent): Promise<void>;
  /** Dispatch a command to the guest. */
  callCommand(
    cmd: string,
    params: unknown,
    options?: BrowserPluginCallOptions,
  ): Promise<CommandResponse>;
  /** Fetch raw HTML for a plugin-owned iframe component. */
  getComponentHtml(componentId: string): Promise<string>;
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

/** Options for building host functions with permission support. */
export interface HostFunctionOptions {
  /** Dynamic plugin ID for permission checks. */
  getPluginId: () => string;
  /** Dynamic plugin display name for permission banners. */
  getPluginName: () => string;
  /** Callback to get current workspace plugin config. Returns undefined if not available. */
  getPluginsConfig?: () => Record<string, PluginConfig> | undefined;
  /** Provider of user-selected files by key name (e.g. from file picker). */
  getFile?: (key: string) => Promise<Uint8Array | null>;
}

const MIN_HTTP_TIMEOUT_MS = 1_000;
const MAX_HTTP_TIMEOUT_MS = 300_000;
const MIN_SUPPORTED_PROTOCOL_VERSION = 1;
const CURRENT_PROTOCOL_VERSION = 1;
/** Maximum storage per key per plugin (1 MiB), matching native DEFAULT_STORAGE_QUOTA_BYTES. */
const STORAGE_QUOTA_BYTES = 1024 * 1024;
/** Maximum cross-plugin command call depth to prevent infinite recursion. */
const MAX_PLUGIN_COMMAND_DEPTH = 8;
let pluginCommandDepth = 0;

/** Reject HTTP headers containing forbidden characters (newlines, null bytes). */
function validateHttpHeaders(headers: Record<string, string>): void {
  for (const [name, value] of Object.entries(headers)) {
    if (
      /[\r\n\0]/.test(name) ||
      /[\r\n\0]/.test(value)
    ) {
      throw new Error(
        `Invalid HTTP header: name or value contains forbidden characters (header: '${name}')`,
      );
    }
  }
}

function resolveHttpTimeoutMs(timeoutMs: unknown): number | null {
  if (timeoutMs == null) return null;
  if (typeof timeoutMs !== "number" || !Number.isFinite(timeoutMs)) {
    return null;
  }
  const normalized = Math.trunc(timeoutMs);
  if (normalized < MIN_HTTP_TIMEOUT_MS) return MIN_HTTP_TIMEOUT_MS;
  if (normalized > MAX_HTTP_TIMEOUT_MS) return MAX_HTTP_TIMEOUT_MS;
  return normalized;
}

function formatLocalRfc3339(date: Date = new Date()): string {
  const pad = (value: number) => value.toString().padStart(2, "0");
  const year = date.getFullYear();
  const month = pad(date.getMonth() + 1);
  const day = pad(date.getDate());
  const hour = pad(date.getHours());
  const minute = pad(date.getMinutes());
  const second = pad(date.getSeconds());
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const offsetHours = pad(Math.floor(absoluteOffset / 60));
  const offsetRemainder = pad(absoluteOffset % 60);
  return `${year}-${month}-${day}T${hour}:${minute}:${second}${sign}${offsetHours}:${offsetRemainder}`;
}

function base64ToBytes(base64: string): Uint8Array<ArrayBuffer> {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

function throwHostFsError(action: string, error: unknown): never {
  const detail = error instanceof Error ? error.message : String(error);
  throw new Error(`${action} failed: ${detail}`);
}

function isMissingWorkspacePathError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes("NotFound") ||
    message.includes("not found") ||
    message.includes("could not be found") ||
    message.includes("object can not be found")
  );
}

function extractWorkspaceUpdateBase64(result: unknown): string | null {
  if (!result || typeof result !== "object") {
    return null;
  }

  const record = result as Record<string, unknown>;
  if (typeof record.data === "string" && record.data.length > 0) {
    return record.data;
  }

  if (record.data && typeof record.data === "object") {
    const nested = record.data as Record<string, unknown>;
    if (typeof nested.data === "string" && nested.data.length > 0) {
      return nested.data;
    }
  }

  return null;
}

function extractWorkspaceUpdateCommandType(command: unknown): string | null {
  if (!command || typeof command !== "object") {
    return null;
  }

  const record = command as Record<string, unknown>;
  if (typeof record.type !== "string") {
    return null;
  }

  if (record.type === "PluginCommand") {
    const params = record.params;
    if (!params || typeof params !== "object") {
      return null;
    }
    const innerCommand = (params as Record<string, unknown>).command;
    return typeof innerCommand === "string" ? innerCommand : null;
  }

  return record.type;
}

async function getRuntimeContextSnapshot(): Promise<Record<string, unknown>> {
  const [
    authModule,
    workspaceRegistryModule,
  ] = await Promise.all([
    import("$lib/auth"),
    import("$lib/storage/localWorkspaceRegistry.svelte"),
  ]);

  const authState = authModule.getAuthState();
  const currentWorkspaceId = workspaceRegistryModule.getCurrentWorkspaceId();
  const currentWorkspace = currentWorkspaceId
    ? workspaceRegistryModule.getLocalWorkspace(currentWorkspaceId)
    : null;
  const providerLinks = currentWorkspaceId
    ? workspaceRegistryModule.getWorkspaceProviderLinks(currentWorkspaceId)
    : [];

  let guestMode = false;
  try {
    const backend = getBackendSync() as { isGuestMode?: () => Promise<boolean> };
    guestMode = (await backend.isGuestMode?.()) ?? false;
  } catch {
    guestMode = false;
  }

  return {
    server_url: authModule.getServerUrl() ?? authState.serverUrl ?? null,
    // Internal: full auth token for host-side use (sync transport, init payload).
    // Redacted to null when exposed to plugins via host_get_runtime_context.
    _auth_token: authModule.getToken() ?? null,
    tier: authState.tier ?? null,
    guest_mode: guestMode,
    current_workspace: currentWorkspace
      ? {
          local_id: currentWorkspace.id,
          name: currentWorkspace.name,
          path: currentWorkspace.path ?? null,
          plugin_metadata: currentWorkspace.pluginMetadata ?? {},
          provider_links: providerLinks.map((link: {
            pluginId: string;
            remoteWorkspaceId: string;
            syncEnabled: boolean;
          }) => ({
            plugin_id: link.pluginId,
            remote_workspace_id: link.remoteWorkspaceId,
            sync_enabled: link.syncEnabled,
          })),
        }
      : null,
  };
}

function normalizeWorkspaceRoot(path: string | null | undefined): string | null {
  const trimmed = path?.trim();
  if (!trimmed) return null;

  if (trimmed.endsWith("/index.md") || trimmed.endsWith("/README.md")) {
    return trimmed.slice(0, trimmed.lastIndexOf("/")) || ".";
  }

  return trimmed;
}

/**
 * Resolve a workspace directory path to its root index file.
 *
 * Uses the backend's `FindRootIndex` command which finds the `.md` file with
 * `contents` but no `part_of` — the canonical root index detection from
 * `diaryx_core::workspace::Workspace::find_root_index_in_dir`.
 *
 * Falls back to the original path if resolution fails.
 */
async function resolveWorkspaceRootIndex(dirPath: string): Promise<string> {
  try {
    const backend = getBackendSync();
    const response: any = await backend.execute({
      type: "FindRootIndex",
      params: { directory: dirPath },
    });
    const resolved = response?.data;
    if (typeof resolved === "string" && resolved.trim()) {
      const result = resolved.trim().replace(/^\.\/+/, "");
      console.debug("[extism] resolveWorkspaceRootIndex:", dirPath, "→", result);
      return result;
    }
    console.warn("[extism] resolveWorkspaceRootIndex: no data in response", response);
  } catch (e) {
    console.warn("[extism] resolveWorkspaceRootIndex failed:", e);
  }
  return dirPath;
}

function readPluginWorkspaceId(
  runtime: Record<string, unknown>,
  pluginId: string,
): string | null {
  const currentWorkspace =
    runtime.current_workspace && typeof runtime.current_workspace === "object"
      ? runtime.current_workspace as Record<string, unknown>
      : null;
  const providerLinks = Array.isArray(currentWorkspace?.provider_links)
    ? currentWorkspace.provider_links as Array<Record<string, unknown>>
    : [];

  for (const link of providerLinks) {
    if (
      link.plugin_id === pluginId &&
      typeof link.remote_workspace_id === "string" &&
      link.remote_workspace_id.trim().length > 0
    ) {
      return link.remote_workspace_id;
    }
  }

  const pluginMetadata =
    currentWorkspace?.plugin_metadata
    && typeof currentWorkspace.plugin_metadata === "object"
      ? currentWorkspace.plugin_metadata as Record<string, unknown>
      : null;
  const metadata =
    pluginMetadata?.[pluginId]
    && typeof pluginMetadata[pluginId] === "object"
      ? pluginMetadata[pluginId] as Record<string, unknown>
        : null;

  const remoteWorkspaceId = metadata?.remoteWorkspaceId;
  if (typeof remoteWorkspaceId === "string" && remoteWorkspaceId.trim().length > 0) {
    return remoteWorkspaceId;
  }

  return typeof metadata?.serverId === "string" && metadata.serverId.trim().length > 0
    ? metadata.serverId
    : null;
}

async function buildBrowserPluginInitPayload(pluginId: string): Promise<Record<string, unknown>> {
  const runtime = await getRuntimeContextSnapshot();
  const currentWorkspace =
    runtime.current_workspace && typeof runtime.current_workspace === "object"
      ? runtime.current_workspace as Record<string, unknown>
      : null;

  let workspaceRoot = normalizeWorkspaceRoot(
    typeof currentWorkspace?.path === "string" ? currentWorkspace.path : null,
  );

  if (!workspaceRoot) {
    try {
      const backend = getBackendSync() as { getWorkspacePath?: () => string };
      workspaceRoot = normalizeWorkspaceRoot(backend.getWorkspacePath?.() ?? null);
    } catch {
      workspaceRoot = null;
    }
  }

  // Resolve directory-only paths (e.g. ".") to the actual root index file
  // so plugins get a valid file path for reading/writing workspace config.
  if (workspaceRoot && !workspaceRoot.endsWith(".md")) {
    workspaceRoot = await resolveWorkspaceRootIndex(workspaceRoot);
  }

  return {
    workspace_root: workspaceRoot,
    workspace_id: readPluginWorkspaceId(runtime, pluginId),
    write_to_disk: true,
    server_url: typeof runtime.server_url === "string" ? runtime.server_url : null,
    auth_token: typeof runtime._auth_token === "string" ? runtime._auth_token : null,
  };
}

interface LoadBrowserPluginOptions {
  initializeLifecycle?: boolean;
}

interface SyncTransportConnectRequest {
  type: "connect";
  server_url: string;
  workspace_id: string;
  auth_token?: string;
  session_code?: string;
  write_to_disk?: boolean;
}

interface SyncTransportSendBinaryRequest {
  type: "send_binary";
  data: string;
}

interface SyncTransportSendTextRequest {
  type: "send_text";
  text: string;
}

interface SyncTransportDisconnectRequest {
  type: "disconnect";
}

type SyncTransportRequest =
  | SyncTransportConnectRequest
  | SyncTransportSendBinaryRequest
  | SyncTransportSendTextRequest
  | SyncTransportDisconnectRequest;

interface SyncTransportCallbacks {
  invokeBinaryExport(exportName: string, input: Uint8Array): Promise<void>;
}

function emitPluginHostEvent(event: unknown): void {
  const backend = getBackendSync();
  if (!backend) return;
  if (
    event &&
    typeof event === "object" &&
    typeof (event as { type?: unknown }).type === "string"
  ) {
    const type = (event as { type: string }).type;
    if (/^[A-Z]/.test(type)) {
      backend.emitFileSystemEvent?.(event as FileSystemEvent);
      return;
    }
  }
}

class BrowserSyncTransportController {
  private callbacks: SyncTransportCallbacks | null = null;
  private ws: WebSocket | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempt = 0;
  private readonly maxReconnectDelay = 30_000;
  private shouldReconnect = false;

  private connectionKey: string | null = null;
  private connectRequest: SyncTransportConnectRequest | null = null;
  private openWaiters: Array<(connected: boolean) => void> = [];

  bindCallbacks(callbacks: SyncTransportCallbacks): void {
    this.callbacks = callbacks;
  }

  async handleRequest(rawRequest: string): Promise<string> {
    const request = JSON.parse(rawRequest) as SyncTransportRequest;
    switch (request.type) {
      case "connect":
        await this.connect(request);
        return JSON.stringify({ ok: true });
      case "send_binary":
        this.sendBinary(base64ToBytes(request.data));
        return JSON.stringify({ ok: true });
      case "send_text":
        this.sendText(request.text);
        return JSON.stringify({ ok: true });
      case "disconnect":
        await this.disconnect();
        return JSON.stringify({ ok: true });
      default:
        return JSON.stringify({ ok: false, error: "unknown_request" });
    }
  }

  async dispose(): Promise<void> {
    await this.disconnect();
    this.callbacks = null;
  }

  /** Return the workspace_id from the active connection, or null. */
  getConnectedWorkspaceId(): string | null {
    return this.connectRequest?.workspace_id ?? null;
  }

  isOpen(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  async ensureConnected(
    request: SyncTransportConnectRequest,
    timeoutMs = 5_000,
  ): Promise<boolean> {
    await this.connect(request);
    if (this.isOpen()) {
      return true;
    }

    return await new Promise<boolean>((resolve) => {
      const timeout = setTimeout(() => {
        this.openWaiters = this.openWaiters.filter((waiter) => waiter !== onResult);
        resolve(this.isOpen());
      }, timeoutMs);

      const onResult = (connected: boolean) => {
        clearTimeout(timeout);
        resolve(connected);
      };

      this.openWaiters.push(onResult);
    });
  }

  private normalizeServerBase(serverUrl: string): string {
    let base = serverUrl.trim().replace(/\/+$/, "");
    while (
      /\/(?:sync2|sync)$/.test(base) ||
      /\/(?:ns|namespaces)\/[^/]+\/sync$/.test(base)
    ) {
      if (/\/(?:sync2|sync)$/.test(base)) {
        base = base.replace(/\/(?:sync2|sync)$/, "");
        continue;
      }
      base = base.replace(/\/(?:ns|namespaces)\/[^/]+\/sync$/, "");
    }
    return base;
  }

  private buildAbsoluteWebSocketBase(serverUrl: string): URL {
    const isAbsolute = /^[a-z][a-z0-9+.-]*:\/\//i.test(serverUrl);
    const url = isAbsolute
      ? new URL(serverUrl)
      : new URL(serverUrl || "/", window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    return url;
  }

  private buildConnectionKey(request: SyncTransportConnectRequest): string {
    return [
      request.server_url.trim(),
      request.workspace_id.trim(),
      request.auth_token ?? "",
      request.session_code ?? "",
      request.write_to_disk === false ? "0" : "1",
    ].join("|");
  }

  private buildWebSocketUrl(request: SyncTransportConnectRequest): string {
    const base = this.normalizeServerBase(request.server_url);
    const url = this.buildAbsoluteWebSocketBase(base);
    url.pathname = `${url.pathname.replace(/\/+$/, "")}/ns/${encodeURIComponent(request.workspace_id)}/sync`;
    url.searchParams.set("workspace_id", request.workspace_id);
    if (request.auth_token) {
      url.searchParams.set("token", request.auth_token);
    }
    if (request.session_code) {
      url.searchParams.set("session", request.session_code);
    }
    return url.toString();
  }

  private async invokeBinaryExport(
    exportName: string,
    input: Uint8Array,
  ): Promise<void> {
    if (!this.callbacks) return;
    await this.callbacks.invokeBinaryExport(exportName, input);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private async connect(request: SyncTransportConnectRequest): Promise<void> {
    const nextKey = this.buildConnectionKey(request);
    if (
      this.connectionKey === nextKey &&
      this.ws &&
      (this.ws.readyState === WebSocket.OPEN ||
        this.ws.readyState === WebSocket.CONNECTING)
    ) {
      return;
    }

    await this.disconnect(false);

    this.shouldReconnect = true;
    this.connectRequest = request;
    this.connectionKey = nextKey;
    this.reconnectAttempt = 0;
    this.openWebSocket(request);
  }

  private openWebSocket(request: SyncTransportConnectRequest): void {
    const ws = new WebSocket(this.buildWebSocketUrl(request));
    ws.binaryType = "arraybuffer";
    this.ws = ws;

    ws.onopen = () => {
      this.reconnectAttempt = 0;
      const waiters = this.openWaiters.splice(0);
      for (const waiter of waiters) {
        waiter(true);
      }
      void this.invokeBinaryExport(
        "on_connected",
        new TextEncoder().encode(
          JSON.stringify({
            workspace_id: request.workspace_id,
            write_to_disk: request.write_to_disk ?? true,
          }),
        ),
      );
    };

    ws.onmessage = (event: MessageEvent) => {
      if (event.data instanceof ArrayBuffer) {
        // DEBUG: log doc_id from v2 frame
        const arr = new Uint8Array(event.data);
        if (arr.length > 1) {
          const idLen = arr[0];
          if (idLen > 0 && arr.length >= 1 + idLen) {
            const docId = new TextDecoder().decode(arr.slice(1, 1 + idLen));
            if (docId.startsWith("body:") || docId.startsWith("workspace:")) {
              console.debug(`[ws:recv] doc_id=${docId} bytes=${arr.length}`);
            }
          }
        }
        void this.invokeBinaryExport(
          "handle_binary_message",
          new Uint8Array(event.data),
        );
        return;
      }
      if (typeof event.data === "string") {
        void this.invokeBinaryExport(
          "handle_text_message",
          new TextEncoder().encode(event.data),
        );
      }
    };

    ws.onerror = (event) => {
      console.error("[extism] browser sync transport error:", event);
    };

    ws.onclose = () => {
      if (this.ws === ws) {
        this.ws = null;
      }

      void this.invokeBinaryExport("on_disconnected", new Uint8Array());
      if (this.shouldReconnect && this.connectRequest) {
        this.scheduleReconnect(this.connectRequest);
      }
    };
  }

  private scheduleReconnect(request: SyncTransportConnectRequest): void {
    this.clearReconnectTimer();
    this.reconnectAttempt += 1;
    const delay = Math.min(
      1000 * Math.pow(1.5, this.reconnectAttempt - 1),
      this.maxReconnectDelay,
    );
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.shouldReconnect) {
        this.openWebSocket(request);
      }
    }, delay);
  }

  private sendBinary(data: Uint8Array<ArrayBuffer>): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      // DEBUG: log doc_id from v2 frame
      if (data.length > 1) {
        const idLen = data[0];
        if (idLen > 0 && data.length >= 1 + idLen) {
          const docId = new TextDecoder().decode(data.slice(1, 1 + idLen));
          if (docId.startsWith("body:") || docId.startsWith("workspace:")) {
            console.debug(`[ws:send] doc_id=${docId} bytes=${data.length}`);
          }
        }
      }
      this.ws.send(data);
    }
  }

  private sendText(text: string): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(text);
    }
  }

  private async disconnect(clearState = true): Promise<void> {
    this.shouldReconnect = false;
    this.clearReconnectTimer();

    const ws = this.ws;
    this.ws = null;

    if (ws) {
      ws.onopen = null;
      ws.onmessage = null;
      ws.onerror = null;
      ws.onclose = null;
      ws.close();
    }

    if (clearState) {
      this.connectionKey = null;
      this.connectRequest = null;
      this.reconnectAttempt = 0;
    }

    if (!this.shouldReconnect) {
      const waiters = this.openWaiters.splice(0);
      for (const waiter of waiters) {
        waiter(false);
      }
    }
  }
}

/**
 * Check a permission, showing a banner if needed.
 * Returns true if allowed, throws an error string if denied.
 */
async function checkBrowserPermission(
  opts: HostFunctionOptions,
  permType: PermissionType,
  target: string,
): Promise<void> {
  const pluginId = opts.getPluginId();
  if (!pluginId || pluginId === "unknown-plugin") {
    throw new Error(
      JSON.stringify({
        error: "permission_checker_unbound_plugin",
        permission: permType,
        target,
      }),
    );
  }
  const pluginsConfig = opts.getPluginsConfig?.();
  const allowed = await permissionStore.requestPermission(
    pluginId,
    opts.getPluginName(),
    permType,
    target,
    pluginsConfig,
  );
  if (!allowed) {
    throw new Error(
      JSON.stringify({
        error: "permission_denied",
        permission: permType,
        target,
        plugin: pluginId,
      }),
    );
  }
}

async function requirePermission(
  opts: HostFunctionOptions | undefined,
  permType: PermissionType,
  target: string,
): Promise<void> {
  if (!opts) {
    throw new Error(
      JSON.stringify({
        error: "permission_checker_missing",
        permission: permType,
        target,
      }),
    );
  }

  // All plugins may access the sync server without explicit permission.
  // The server authenticates via session cookies, so this is safe.
  if (permType === "http_requests") {
    try {
      const authModule = await import("$lib/auth");
      const serverUrl = authModule.getServerUrl();
      if (serverUrl) {
        const serverOrigin = new URL(serverUrl).origin;
        const targetOrigin = new URL(target).origin;
        if (targetOrigin === serverOrigin) return;
      }
    } catch {}
  }

  await checkBrowserPermission(opts, permType, target);
}

function buildHostFunctions(
  transport: BrowserSyncTransportController,
  opts?: HostFunctionOptions,
) {
  /** Encode a key that may contain `/` for use in a URL path, encoding each segment individually. */
  function encodeKeyPath(key: string): string {
    return key.split("/").map(encodeURIComponent).join("/");
  }

  async function getNamespaceServerBase(): Promise<string> {
    const authModule = await import("$lib/auth");
    const serverUrl = authModule.getServerUrl();
    if (!serverUrl) {
      throw new Error("not authenticated");
    }
    return serverUrl.replace(/\/$/, "");
  }

  async function namespaceFetch(
    method: string,
    path: string,
    init: Omit<RequestInit, "method" | "credentials"> = {},
    okStatuses: number[] = [],
  ): Promise<Response> {
    const serverBase = await getNamespaceServerBase();
    const response = await fetch(`${serverBase}${path}`, {
      ...init,
      method,
      credentials: "include",
    });

    if (!response.ok && !okStatuses.includes(response.status)) {
      const text = await response.text();
      throw new Error(text || `${method} returned ${response.status}`);
    }

    return response;
  }

  return {
    "extism:host/user": {
      host_log(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { level: string; message: string }
            | undefined;
          if (!input) return cp.store("");
          const pluginId = opts?.getPluginId?.() ?? "unknown-plugin";
          const prefix = `[extism-plugin:${pluginId}]`;
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
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "read_files", path);
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "ReadFile",
            params: { path },
          } as any);
          if (response.type === "String") {
            return cp.store(response.data);
          }
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_read_binary(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { path: string } | undefined;
          if (!input) return cp.store("");
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "read_files", path);
          const backend = getBackendSync();
          const data = await backend.readBinary(path);
          return cp.store(JSON.stringify({ data: bytesToBase64(data) }));
        } catch {
          return cp.store("");
        }
      },
      async host_list_files(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { prefix: string } | undefined;
          if (!input) return cp.store("[]");
          const prefix = normalizeExtismHostPath(
            typeof input.prefix === "string" ? input.prefix : "",
          );
          await requirePermission(opts, "read_files", prefix);
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
          return cp.store(JSON.stringify(collectFilesystemTreePaths(response.data)));
        } catch {
          return cp.store("[]");
        }
      },
      async host_workspace_file_set(cp: CallContext, _offs: bigint) {
        try {
          const runtime = await getRuntimeContextSnapshot();
          const currentWorkspace =
            runtime.current_workspace && typeof runtime.current_workspace === "object"
              ? runtime.current_workspace as Record<string, unknown>
              : null;
          const workspacePath = normalizeWorkspaceRoot(
            typeof currentWorkspace?.path === "string" ? currentWorkspace.path : null,
          );
          if (!workspacePath) return cp.store("[]");
          await requirePermission(opts, "read_files", workspacePath);

          const backend = getBackendSync();
          const rootIndex = workspacePath.endsWith(".md")
            ? workspacePath
            : await resolveWorkspaceRootIndex(workspacePath);
          const response: any = await backend.execute({
            type: "GetWorkspaceFileSet",
            params: { path: rootIndex },
          } as any);
          if (response.type !== "Strings") {
            return cp.store("[]");
          }
          return cp.store(JSON.stringify(response.data));
        } catch {
          return cp.store("[]");
        }
      },
      async host_file_exists(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { path: string } | undefined;
          if (!input) return cp.store("false");
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "read_files", path);
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "FileExists",
            params: { path },
          } as any);
          if (response.type === "Bool") {
            return cp.store(response.data ? "true" : "false");
          }
          return cp.store("false");
        } catch {
          return cp.store("false");
        }
      },
      async host_file_metadata(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { path: string } | undefined;
          if (!input) {
            return cp.store(JSON.stringify({
              exists: false,
              size_bytes: null,
              modified_at_ms: null,
            }));
          }
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "read_files", path);
          const backend = getBackendSync();
          const response: any = await backend.execute({
            type: "GetFileInfo",
            params: { path },
          } as any);
          if (response.type === "FileInfo") {
            return cp.store(JSON.stringify(response.data));
          }
          return cp.store(JSON.stringify({
            exists: false,
            size_bytes: null,
            modified_at_ms: null,
          }));
        } catch {
          return cp.store(JSON.stringify({
            exists: false,
            size_bytes: null,
            modified_at_ms: null,
          }));
        }
      },
      async host_storage_get(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string } | undefined;
          if (!input?.key) return cp.store("");
          await requirePermission(opts, "plugin_storage", input.key);
          if (!opts) {
            throw new Error(
              JSON.stringify({
                error: "permission_checker_missing",
                permission: "plugin_storage",
                target: input.key,
              }),
            );
          }
          const pluginId = opts.getPluginId();
          const storagePath = getPluginStoragePath(pluginId, input.key);
          const backend = getBackendSync();
          try {
            const bytes = await backend.readBinary(storagePath);
            if (bytes) {
              return cp.store(JSON.stringify({ data: bytesToBase64(bytes) }));
            }
          } catch (error) {
            if (!isMissingWorkspacePathError(error)) {
              throw error;
            }
          }

          // Migration path for pre-workspace browser plugin storage.
          const legacyKey = `diaryx-plugin:${pluginId}:${input.key}`;
          const raw = localStorage.getItem(legacyKey);
          if (!raw) return cp.store("");
          try {
            const parsed = JSON.parse(raw) as { data?: string };
            if (typeof parsed.data === "string" && parsed.data.length > 0) {
              await backend.writeBinary(storagePath, base64ToBytes(parsed.data));
            }
          } catch {
            // Ignore malformed legacy state and return the raw value for compatibility.
          }
          return cp.store(raw);
        } catch {
          return cp.store("");
        }
      },
      async host_storage_set(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { key: string; data: string }
            | undefined;
          if (!input?.key) return cp.store("");
          await requirePermission(opts, "plugin_storage", input.key);
          if (!opts) {
            throw new Error(
              JSON.stringify({
                error: "permission_checker_missing",
                permission: "plugin_storage",
                target: input.key,
              }),
            );
          }
          const pluginId = opts.getPluginId();
          const bytes = base64ToBytes(input.data);
          if (bytes.length > STORAGE_QUOTA_BYTES) {
            throw new Error(
              `host_storage_set: data size (${bytes.length} bytes) exceeds plugin storage quota (${STORAGE_QUOTA_BYTES} bytes)`,
            );
          }
          const backend = getBackendSync();
          await backend.writeBinary(
            getPluginStoragePath(pluginId, input.key),
            bytes,
          );
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_secret_get(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string } | undefined;
          if (!input?.key) return cp.store("");
          await requirePermission(opts, "plugin_storage", input.key);
          if (!opts) {
            throw new Error(
              JSON.stringify({
                error: "permission_checker_missing",
                permission: "plugin_storage",
                target: input.key,
              }),
            );
          }
          const pluginId = opts.getPluginId();
          const value = await getPluginSecret(pluginId, input.key);
          if (!value) return cp.store("");
          return cp.store(JSON.stringify({ value }));
        } catch {
          return cp.store("");
        }
      },
      async host_secret_set(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { key: string; value: string }
            | undefined;
          if (!input?.key) return cp.store("");
          await requirePermission(opts, "plugin_storage", input.key);
          if (!opts) {
            throw new Error(
              JSON.stringify({
                error: "permission_checker_missing",
                permission: "plugin_storage",
                target: input.key,
              }),
            );
          }
          const pluginId = opts.getPluginId();
          await setPluginSecret(pluginId, input.key, input.value ?? "");
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_secret_delete(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as { key: string } | undefined;
          if (!input?.key) return cp.store("");
          await requirePermission(opts, "plugin_storage", input.key);
          if (!opts) {
            throw new Error(
              JSON.stringify({
                error: "permission_checker_missing",
                permission: "plugin_storage",
                target: input.key,
              }),
            );
          }
          const pluginId = opts.getPluginId();
          await deletePluginSecret(pluginId, input.key);
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      host_get_timestamp(cp: CallContext, _offs: bigint) {
        return cp.store(Date.now().toString());
      },
      host_get_now(cp: CallContext, _offs: bigint) {
        return cp.store(formatLocalRfc3339());
      },
      async host_http_request(cp: CallContext, offs: bigint) {
        let timeoutMs: number | null = null;
        try {
          const input = cp.read(offs)?.json() as
            | {
                url: string;
                method: string;
                headers: Record<string, string>;
                body?: string;
                body_base64?: string;
                timeout_ms?: number;
              }
            | undefined;
          if (!input)
            return cp.store(
              JSON.stringify({ status: 0, headers: {}, body: "no input" }),
            );
          await requirePermission(opts, "http_requests", input.url);
          validateHttpHeaders(input.headers ?? {});
          timeoutMs = resolveHttpTimeoutMs(input.timeout_ms);
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
          const abortController =
            timeoutMs !== null && typeof AbortController === "function"
              ? new AbortController()
              : null;
          const timeoutId =
            abortController !== null && timeoutMs !== null
              ? globalThis.setTimeout(() => abortController.abort(), timeoutMs)
              : null;
          let respStatus = 0;
          let respHeaders: Record<string, string> = {};
          let bytes: Uint8Array;
          try {
            // Include credentials (cookies) for same-origin and sync server
            // requests, but not for third-party CDNs (which reject credentials
            // when Access-Control-Allow-Origin is wildcard).
            const requestUrl = new URL(input.url);
            const isSameOrigin = requestUrl.origin === globalThis.location?.origin;
            let isServerUrl = false;
            try {
              const authModule = await import("$lib/auth");
              const sUrl = authModule.getServerUrl();
              if (sUrl) isServerUrl = requestUrl.origin === new URL(sUrl).origin;
            } catch {}
            const credentials = isSameOrigin || isServerUrl ? "include" as const : "omit" as const;

            const resp = await proxyFetch(input.url, {
              method: input.method,
              headers: input.headers,
              body: fetchBody,
              credentials,
              signal: abortController?.signal,
              timeout_ms: timeoutMs ?? undefined,
            });
            respStatus = resp.status;
            resp.headers.forEach((v, k) => {
              respHeaders[k] = v;
            });
            // Read the full response body while still under abort timeout
            // coverage. Previously the timeout was cleared after headers
            // arrived, leaving arrayBuffer() with no timeout — causing sync
            // pull to hang indefinitely when body transfer stalled.
            bytes = new Uint8Array(await resp.arrayBuffer());
          } finally {
            if (timeoutId !== null) {
              globalThis.clearTimeout(timeoutId);
            }
          }
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
              status: respStatus,
              headers: respHeaders,
              body,
              body_base64,
            }),
          );
        } catch (e) {
          const msg =
            e instanceof Error && e.name === "AbortError" && timeoutMs !== null
              ? `Request timed out after ${timeoutMs}ms`
              : e instanceof Error
                ? e.message
                : String(e);
          return cp.store(
            JSON.stringify({ status: 0, headers: {}, body: msg }),
          );
        }
      },
      async host_write_file(cp: CallContext, offs: bigint) {
        const input = cp.read(offs)?.json() as
          | { path: string; content: string }
          | undefined;
        if (!input) return cp.store("");
        try {
          const path = normalizeExtismHostPath(input.path);
          const backend = getBackendSync();
          const existsResp: any = await backend.execute({
            type: "FileExists",
            params: { path },
          } as any);
          const exists = existsResp?.type === "Bool" && !!existsResp.data;
          await requirePermission(
            opts,
            exists ? "edit_files" : "create_files",
            path,
          );
          await backend.execute({
            type: "WriteFile",
            params: { path, content: input.content },
          } as any);
        } catch (error) {
          throwHostFsError("host_write_file", error);
        }
        return cp.store("");
      },
      async host_delete_file(cp: CallContext, offs: bigint) {
        const input = cp.read(offs)?.json() as
          | { path: string }
          | undefined;
        if (!input) return cp.store("");
        try {
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "delete_files", path);
          const backend = getBackendSync();
          await backend.execute({
            type: "DeleteFile",
            params: { path },
          } as any);
        } catch (error) {
          throwHostFsError("host_delete_file", error);
        }
        return cp.store("");
      },
      async host_write_binary(cp: CallContext, offs: bigint) {
        const input = cp.read(offs)?.json() as
          | { path: string; content: string }
          | undefined;
        if (!input) return cp.store("");
        try {
          const path = normalizeExtismHostPath(input.path);
          const backend = getBackendSync();
          const existsResp: any = await backend.execute({
            type: "FileExists",
            params: { path },
          } as any);
          const exists = existsResp?.type === "Bool" && !!existsResp.data;
          await requirePermission(
            opts,
            exists ? "edit_files" : "create_files",
            path,
          );
          // Decode base64 to Uint8Array
          const binary = atob(input.content);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) {
            bytes[i] = binary.charCodeAt(i);
          }
          await backend.writeBinary(path, bytes);
        } catch (error) {
          throwHostFsError("host_write_binary", error);
        }
        return cp.store("");
      },
      host_emit_event(cp: CallContext, offs: bigint) {
        try {
          const eventJson = cp.read(offs)?.text();
          if (!eventJson) return cp.store("");
          const event = JSON.parse(eventJson);
          emitPluginHostEvent(event);
          return cp.store("");
        } catch {
          return cp.store("");
        }
      },
      async host_request_file(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { key: string }
            | undefined;
          if (!input?.key) return cp.store("");
          if (!opts?.getFile) return cp.store("");
          const bytes = await opts.getFile(input.key);
          if (!bytes) return cp.store("");
          return cp.store(bytes);
        } catch {
          return cp.store("");
        }
      },
      async host_plugin_command(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | {
                plugin_id?: string;
                command?: string;
                params?: unknown;
              }
            | undefined;
          const targetPluginId = input?.plugin_id?.trim();
          const command = input?.command?.trim();
          if (!targetPluginId || !command) {
            return cp.store(
              JSON.stringify({
                success: false,
                error: "plugin_id and command are required",
              }),
            );
          }

          const callerPluginId = opts?.getPluginId?.() ?? "unknown-plugin";
          if (targetPluginId === callerPluginId) {
            return cp.store(
              JSON.stringify({
                success: false,
                error:
                  "Plugins cannot call their own commands via host_plugin_command",
              }),
            );
          }

          if (pluginCommandDepth >= MAX_PLUGIN_COMMAND_DEPTH) {
            return cp.store(
              JSON.stringify({
                success: false,
                error: `Cross-plugin command call depth limit exceeded (max ${MAX_PLUGIN_COMMAND_DEPTH})`,
              }),
            );
          }

          await requirePermission(
            opts,
            "execute_commands",
            `${targetPluginId}:${command}`,
          );

          pluginCommandDepth += 1;
          try {
            const result = await dispatchCommand(
              targetPluginId,
              command,
              input?.params ?? {},
            );
            return cp.store(JSON.stringify(result));
          } finally {
            pluginCommandDepth = Math.max(0, pluginCommandDepth - 1);
          }
        } catch (e) {
          return cp.store(
            JSON.stringify({
              success: false,
              error: e instanceof Error ? e.message : String(e),
            }),
          );
        }
      },
      async host_get_runtime_context(cp: CallContext, _offs: bigint) {
        try {
          const snapshot = await getRuntimeContextSnapshot();
          // Redact internal auth token — plugins should use host_proxy_request
          // or host_http_request which handle credentials automatically.
          const { _auth_token, ...safeSnapshot } = snapshot;
          return cp.store(JSON.stringify(safeSnapshot));
        } catch {
          return cp.store(JSON.stringify({}));
        }
      },
      async host_ws_request(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.text();
          if (!input) {
            return cp.store(JSON.stringify({ ok: false, error: "missing_input" }));
          }
          return cp.store(await transport.handleRequest(input));
        } catch (e) {
          const error = e instanceof Error ? e.message : String(e);
          return cp.store(JSON.stringify({ ok: false, error }));
        }
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
          await requirePermission(opts, "plugin_storage", input.module_key);
          if (!opts) {
            throw new Error("host_run_wasi_module: missing plugin identity");
          }
          const result = await handleHostRunWasiModule(
            input,
            opts.getPluginId(),
          );
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
      async host_namespace_put_object(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | {
                ns_id: string;
                key: string;
                body_base64: string;
                mime_type: string;
                audience: string;
              }
            | undefined;
          if (!input) return cp.store(JSON.stringify({ error: "no input" }));
          const binary = atob(input.body_base64);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) {
            bytes[i] = binary.charCodeAt(i);
          }
          await namespaceFetch(
            "PUT",
            `/namespaces/${encodeURIComponent(input.ns_id)}/objects/${encodeKeyPath(input.key)}`,
            {
              headers: {
                "Content-Type": input.mime_type,
                "X-Audience": input.audience,
              },
              body: bytes,
            },
          );
          return cp.store(JSON.stringify({ ok: true }));
        } catch (e) {
          return cp.store(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
        }
      },
      async host_namespace_delete_object(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { ns_id: string; key: string }
            | undefined;
          if (!input) return cp.store(JSON.stringify({ error: "no input" }));
          await namespaceFetch(
            "DELETE",
            `/namespaces/${encodeURIComponent(input.ns_id)}/objects/${encodeKeyPath(input.key)}`,
            {},
            [404],
          );
          return cp.store(JSON.stringify({ ok: true }));
        } catch (e) {
          return cp.store(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
        }
      },
      async host_namespace_list_objects(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { ns_id: string }
            | undefined;
          if (!input) return cp.store(JSON.stringify([]));
          const resp = await namespaceFetch(
            "GET",
            `/namespaces/${encodeURIComponent(input.ns_id)}/objects`,
          );
          const data = await resp.text();
          return cp.store(data);
        } catch (e) {
          return cp.store(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
        }
      },
      async host_namespace_sync_audience(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { ns_id: string; audience: string; access: string }
            | undefined;
          if (!input) return cp.store(JSON.stringify({ error: "no input" }));
          await namespaceFetch(
            "PUT",
            `/namespaces/${encodeURIComponent(input.ns_id)}/audiences/${encodeURIComponent(input.audience)}`,
            {
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ access: input.access }),
            },
          );
          return cp.store(JSON.stringify({ ok: true }));
        } catch (e) {
          return cp.store(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
        }
      },
      async host_namespace_send_email(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | {
                ns_id: string;
                audience: string;
                subject: string;
                reply_to?: string;
              }
            | undefined;
          if (!input) return cp.store(JSON.stringify({ error: "no input" }));
          const body: Record<string, string> = { subject: input.subject };
          if (input.reply_to) body.reply_to = input.reply_to;
          const resp = await namespaceFetch(
            "POST",
            `/namespaces/${encodeURIComponent(input.ns_id)}/audiences/${encodeURIComponent(input.audience)}/send-email`,
            {
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(body),
            },
          );
          const data = await resp.text();
          return cp.store(data);
        } catch (e) {
          return cp.store(JSON.stringify({ error: e instanceof Error ? e.message : String(e) }));
        }
      },
      async host_proxy_request(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | {
                proxy_id: string;
                path?: string;
                method?: string;
                headers?: Record<string, string>;
                body?: string;
              }
            | undefined;
          if (!input?.proxy_id) {
            return cp.store(
              JSON.stringify({
                status: 0,
                headers: {},
                body: "host_proxy_request: missing proxy_id",
              }),
            );
          }
          if (input.headers) {
            validateHttpHeaders(input.headers);
          }

          // Build proxy URL — same-origin request to the server's proxy endpoint
          const path = (input.path ?? "").replace(/^\//, "");
          const proxyUrl = path
            ? `/api/proxy/${encodeURIComponent(input.proxy_id)}/${path}`
            : `/api/proxy/${encodeURIComponent(input.proxy_id)}`;

          // For non-same-origin deployments, resolve server URL
          let fetchUrl = proxyUrl;
          try {
            const authModule = await import("$lib/auth");
            const serverUrl = authModule.getServerUrl();
            if (serverUrl) {
              const serverOrigin = new URL(serverUrl).origin;
              if (serverOrigin !== globalThis.location?.origin) {
                fetchUrl = `${serverOrigin}${proxyUrl}`;
              }
            }
          } catch {}

          const abortController =
            typeof AbortController === "function"
              ? new AbortController()
              : null;
          const timeoutId =
            abortController !== null
              ? globalThis.setTimeout(() => abortController.abort(), 120_000)
              : null;

          let respStatus = 0;
          let respHeaders: Record<string, string> = {};
          let bytes: Uint8Array;
          try {
            const resp = await fetch(fetchUrl, {
              method: input.method ?? "POST",
              headers: {
                "Content-Type": "application/json",
                ...(input.headers ?? {}),
              },
              body: input.body ?? undefined,
              credentials: "include",
              signal: abortController?.signal,
            });
            respStatus = resp.status;
            resp.headers.forEach((v, k) => {
              respHeaders[k] = v;
            });
            // Read body under timeout coverage (same fix as host_http_request)
            bytes = new Uint8Array(await resp.arrayBuffer());
          } finally {
            if (timeoutId !== null) {
              globalThis.clearTimeout(timeoutId);
            }
          }

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
              status: respStatus,
              headers: respHeaders,
              body,
              body_base64,
            }),
          );
        } catch (e) {
          const msg =
            e instanceof Error && e.name === "AbortError"
              ? "Proxy request timed out"
              : e instanceof Error
                ? e.message
                : String(e);
          return cp.store(
            JSON.stringify({ status: 0, headers: {}, body: msg }),
          );
        }
      },
      async host_hash_file(cp: CallContext, offs: bigint) {
        try {
          const input = cp.read(offs)?.json() as
            | { path: string }
            | undefined;
          if (!input) return cp.store("");
          const path = normalizeExtismHostPath(input.path);
          await requirePermission(opts, "read_files", path);
          const backend = getBackendSync();
          const data = await backend.readBinary(path);
          const hashBuffer = await crypto.subtle.digest("SHA-256", new Uint8Array(data));
          const hashHex = Array.from(new Uint8Array(hashBuffer))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("");
          return cp.store(JSON.stringify({ hash: hashHex }));
        } catch {
          return cp.store("");
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
        case "crdt_commands":
          return "CrdtCommands" as PluginCapability;
        case "sync_transport":
          return "SyncTransport" as PluginCapability;
        case "custom_commands":
          return {
            CustomCommands: { commands: guest.commands },
          } as unknown as PluginCapability;
        case "editor_extension":
          return "EditorExtension" as PluginCapability;
        case "media_transcoder":
          return {
            MediaTranscoder: { conversions: guest.conversions ?? [] },
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
    cli: Array.isArray(guest.cli)
      ? (guest.cli as PluginManifest["cli"])
      : [],
  };
}

function validateProtocolVersion(guest: GuestManifest): void {
  const version = guest.protocol_version ?? 1;
  if (
    version < MIN_SUPPORTED_PROTOCOL_VERSION ||
    version > CURRENT_PROTOCOL_VERSION
  ) {
    throw new Error(
      `Plugin protocol version ${version} is not supported by this browser host ` +
        `(supported: ${MIN_SUPPORTED_PROTOCOL_VERSION}-${CURRENT_PROTOCOL_VERSION})`,
    );
  }
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
 *
 * @param wasmBytes - Raw WASM binary
 * @param hostOpts - Optional permission context. If provided, host functions
 *   will check permissions via the permissionStore before proceeding.
 */
export async function loadBrowserPlugin(
  wasmBytes: ArrayBuffer,
  hostOpts?: HostFunctionOptions,
  options?: LoadBrowserPluginOptions,
): Promise<BrowserExtismPlugin> {
  const support = getBrowserPluginRuntimeSupport();
  if (!support.supported) {
    throw new Error(
      support.reason ?? "Browser plugins are not supported in this runtime.",
    );
  }

  const transport = new BrowserSyncTransportController();
  async function ensureWorkspaceTransportConnection(): Promise<string | null> {
    const runtime = await getRuntimeContextSnapshot();
    const workspaceId = readPluginWorkspaceId(runtime, manifest.id as unknown as string);
    const serverUrl =
      typeof runtime.server_url === "string" && runtime.server_url.trim().length > 0
        ? runtime.server_url
        : null;
    const authToken =
      typeof runtime._auth_token === "string" && runtime._auth_token.trim().length > 0
        ? runtime._auth_token
        : undefined;

    if (!workspaceId || !serverUrl) {
      return null;
    }

    const connected = await transport.ensureConnected({
      type: "connect",
      server_url: serverUrl,
      workspace_id: workspaceId,
      auth_token: authToken,
      write_to_disk: true,
    });

    return connected ? workspaceId : null;
  }

  async function emitWorkspaceUpdateFromCommand(command: unknown, result: unknown): Promise<void> {
    const commandType = extractWorkspaceUpdateCommandType(command);

    if (commandType !== "TriggerWorkspaceSync" && commandType !== "CreateWorkspaceUpdate") {
      return;
    }

    const base64Data = extractWorkspaceUpdateBase64(result);
    if (!base64Data) {
      return;
    }

    // The WASM plugin returns raw SyncMessage bytes. The sync server expects
    // v2-framed messages: [u8: doc_id_len][doc_id_bytes][payload].
    // Frame the workspace update with the proper workspace doc_id before sending.
    let workspaceId = transport.getConnectedWorkspaceId();
    if (!workspaceId || !transport.isOpen()) {
      workspaceId = await ensureWorkspaceTransportConnection();
    }
    console.debug(`[extism] emitWorkspaceUpdateFromCommand: commandType=${commandType} workspaceId=${workspaceId} base64Len=${base64Data.length}`);
    if (!workspaceId) {
      console.warn("[extism] emitWorkspaceUpdateFromCommand: no connected workspace_id, dropping update");
      return;
    }

    const payloadBytes = base64ToBytes(base64Data);

    const docId = `workspace:${workspaceId}`;
    const docIdBytes = new TextEncoder().encode(docId);
    const docIdLen = Math.min(docIdBytes.length, 255);
    const framed = new Uint8Array(1 + docIdLen + payloadBytes.length);
    framed[0] = docIdLen;
    framed.set(docIdBytes.subarray(0, docIdLen), 1);
    framed.set(payloadBytes, 1 + docIdLen);

    // Re-encode as base64 for the transport's send_binary handler
    const framedBase64 = bytesToBase64(framed);
    console.debug(`[extism] emitWorkspaceUpdateFromCommand: sending framed ws update docId=${docId} payloadBytes=${payloadBytes.length} framedBytes=${framed.length} first3=[${payloadBytes[0]},${payloadBytes[1]},${payloadBytes[2]}]`);

    await transport.handleRequest(JSON.stringify({
      type: "send_binary",
      data: framedBase64,
    }));
  }

  let activeCallGetFile: HostFunctionOptions["getFile"] | undefined;
  const effectiveHostOpts = hostOpts
    ? {
        ...hostOpts,
        getFile: async (key: string) => {
          const provider = activeCallGetFile ?? hostOpts.getFile;
          if (!provider) return null;
          return provider(key);
        },
      }
    : undefined;
  const PLUGIN_LOAD_TIMEOUT_MS = 30_000;
  const hostFunctions = buildHostFunctions(transport, effectiveHostOpts);

  let plugin: ExtismPlugin;
  try {
    plugin = await withTimeout(
      createPlugin(wasmBytes, {
        useWasi: true,
        runInWorker: support.useWorkerFallback ?? false,
        functions: hostFunctions,
      }),
      PLUGIN_LOAD_TIMEOUT_MS,
      "Plugin instantiation timed out. This usually means the WASM module " +
        "imports a host function that the browser runtime does not provide. " +
        "Check the browser console for details.",
    );
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    // Log available host functions to help diagnose missing imports
    const availableFns = Object.keys(
      (hostFunctions as Record<string, Record<string, unknown>>)?.["extism:host/user"] ?? {},
    );
    console.error(
      `[extism] Failed to instantiate plugin.\n` +
        `  Error: ${msg}\n` +
        `  Available host functions (${availableFns.length}): ${availableFns.join(", ")}\n` +
        `  If the plugin was recently updated, it may require a newer version of the app.`,
    );
    throw new Error(
      `Failed to load plugin: ${msg}`,
    );
  }

  // Call guest `manifest` export to get the plugin's manifest.
  let manifestOutput: Awaited<ReturnType<ExtismPlugin["call"]>>;
  try {
    manifestOutput = await withTimeout(
      plugin.call("manifest", ""),
      10_000,
      "Plugin manifest() call timed out",
    );
  } catch (err) {
    await plugin.close().catch(() => {});
    throw err;
  }
  if (!manifestOutput) {
    await plugin.close().catch(() => {});
    throw new Error("Plugin manifest() returned null");
  }
  const guestManifest: GuestManifest = manifestOutput.json();
  validateProtocolVersion(guestManifest);

  // Validate that the guest-declared plugin ID matches the expected ID from
  // the host context (if one has already been set from a prior manifest
  // inspection). This prevents a malicious plugin from claiming a different
  // ID to access another plugin's storage or bypass permission rules.
  const expectedId = hostOpts?.getPluginId?.();
  if (
    expectedId &&
    expectedId !== "unknown-plugin" &&
    guestManifest.id !== expectedId
  ) {
    await plugin.close().catch(() => {});
    throw new Error(
      `Plugin ID mismatch: expected '${expectedId}' but guest manifest declares '${guestManifest.id}'`,
    );
  }

  const manifest = convertGuestManifest(guestManifest);

  function extractComponentHtml(value: unknown): string | null {
    if (typeof value === "string") return value;
    if (!value || typeof value !== "object") return null;

    const obj = value as Record<string, unknown>;
    if (typeof obj.response === "string") return obj.response;
    if (typeof obj.html === "string") return obj.html;
    if (typeof obj.data === "string") return obj.data;
    if (obj.type === "PluginResult") {
      return extractComponentHtml(obj.data);
    }
    if (obj.success === true) {
      return extractComponentHtml(obj.data);
    }

    return null;
  }

  function isOptionalExportMissing(
    error: unknown,
    exportName: string,
  ): boolean {
    const message = (
      error instanceof Error
        ? error.message
        : typeof error === "object" && error !== null && "message" in error
          ? String((error as { message?: unknown }).message ?? "")
          : String(error)
    ).toLowerCase();
    return (
      message.includes("function not found") ||
      message.includes("unknown function") ||
      message.includes("does not exist") ||
      message.includes(`no such export`) ||
      (message.includes("not found") && message.includes(exportName))
    );
  }

  // Serialize all calls to the WASM plugin. WASM modules are single-threaded;
  // concurrent plugin.call() invocations cause response mix-ups.
  let callQueue: Promise<unknown> = Promise.resolve();
  let queueDepth = 0;
  function enqueue<T>(label: string, fn: () => Promise<T>): Promise<T> {
    const waitingBefore = queueDepth;
    queueDepth += 1;
    const run = async () => {
      const startedAt = performance.now();
      console.debug(`[extism] ${manifest.id}: ${label} start`, {
        waitingBefore,
      });
      try {
        return await fn();
      } finally {
        queueDepth = Math.max(0, queueDepth - 1);
        console.debug(`[extism] ${manifest.id}: ${label} done`, {
          elapsedMs: Math.round(performance.now() - startedAt),
          queueDepth,
        });
      }
    };
    const next = callQueue.then(run, run);
    callQueue = next.then(
      () => {},
      () => {},
    );
    return next;
  }

  async function withCallOptions<T>(
    callOptions: BrowserPluginCallOptions | undefined,
    fn: () => Promise<T>,
  ): Promise<T> {
    const previousGetFile = activeCallGetFile;
    activeCallGetFile = callOptions?.getFile;
    try {
      return await fn();
    } finally {
      activeCallGetFile = previousGetFile;
    }
  }

  transport.bindCallbacks({
    invokeBinaryExport(exportName: string, input: Uint8Array): Promise<void> {
      return enqueue(`transport:${exportName}`, async () => {
        try {
          await plugin.call(exportName, input);
        } catch (e) {
          console.warn(
            `[extism] ${manifest.id}: transport callback ${exportName} failed:`,
            e,
          );
        }
      });
    },
  });

  const browserPlugin: BrowserExtismPlugin = {
    manifest,
    requestedPermissions: guestManifest.requested_permissions,

    async callLifecycle(
      exportName: "init" | "shutdown",
      payload?: unknown,
    ): Promise<void> {
      return enqueue(`callLifecycle:${exportName}`, async () => {
        const input =
          payload == null
            ? "{}"
            : typeof payload === "string"
              ? payload
              : JSON.stringify(payload);
        try {
          await plugin.call(exportName, input);
        } catch (e) {
          if (isOptionalExportMissing(e, exportName)) return;
          throw e;
        }
      });
    },

    async callEvent(event: GuestEvent): Promise<void> {
      return enqueue("callEvent:on_event", async () => {
        try {
          await plugin.call("on_event", JSON.stringify(event));
        } catch (e) {
          console.warn(`[extism] ${manifest.id}: on_event failed:`, e);
        }
      });
    },

    async callCommand(
      cmd: string,
      params: unknown,
      callOptions?: BrowserPluginCallOptions,
    ): Promise<CommandResponse> {
      return enqueue(`callCommand:${cmd}`, async () => {
        return withCallOptions(callOptions, async () => {
          const request = JSON.stringify({ command: cmd, params });
          try {
            console.debug(`[extism] ${manifest.id}: handle_command request`, {
              command: cmd,
              params,
            });
            const output = await plugin.call("handle_command", request);
            if (!output)
              return { success: false, error: "No response from plugin" };
            const response = output.json() as CommandResponse;
            await emitWorkspaceUpdateFromCommand({ type: cmd }, response);
            console.debug(`[extism] ${manifest.id}: handle_command response`, {
              command: cmd,
              success: response.success,
              hasData: response.data != null,
              error: response.error ?? null,
            });
            return response;
          } catch (e) {
            console.error(`[extism] ${manifest.id}: handle_command failed`, {
              command: cmd,
              error: e instanceof Error ? e.message : String(e),
            });
            return {
              success: false,
              error: e instanceof Error ? e.message : String(e),
            };
          }
        });
      });
    },

    async getComponentHtml(componentId: string): Promise<string> {
      return enqueue("getComponentHtml:get_component_html", async () => {
        const input = JSON.stringify({ component_id: componentId });
        try {
          const output = await plugin.call("get_component_html", input);
          if (!output) {
            throw new Error("No response from plugin");
          }
          const html = output.text();
          if (!html) {
            throw new Error("Plugin returned empty component HTML");
          }
          return html;
        } catch (e) {
          if (!isOptionalExportMissing(e, "get_component_html")) {
            throw e;
          }

          const fallback = await browserPlugin.callCommand("get_component_html", {
            component_id: componentId,
          });
          if (!fallback.success) {
            throw new Error(fallback.error ?? "Failed to load component HTML");
          }

          const html = extractComponentHtml(fallback.data);
          if (!html) {
            throw new Error("Plugin returned invalid component HTML");
          }
          return html;
        }
      });
    },

    async callTypedCommand(command: unknown): Promise<unknown | null> {
      return enqueue("callTypedCommand:execute_typed_command", async () => {
        try {
          const output = await plugin.call(
            "execute_typed_command",
            JSON.stringify(command),
          );
          if (!output) return null;
          const text = output.text();
          if (!text) return null;
          const parsed = JSON.parse(text);
          await emitWorkspaceUpdateFromCommand(command, parsed);
          return parsed;
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          if (message.includes("Plugin state not initialized")) {
            // Expected before sync plugin lifecycle init; caller can fall back.
            console.debug(
              `[extism] ${manifest.id}: execute_typed_command skipped before init`,
            );
            return null;
          }
          // Re-throw so callers get the actual error instead of silently
          // falling through to the WASM backend (which has no sync plugin).
          throw e;
        }
      });
    },

    async callBinary(
      exportName: string,
      data: Uint8Array,
    ): Promise<Uint8Array | null> {
      return enqueue(`callBinary:${exportName}`, async () => {
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
      return enqueue(`callRender:${exportName}`, async () => {
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
      return enqueue("getConfig:get_config", async () => {
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
      return enqueue("setConfig:set_config", async () => {
        try {
          await plugin.call("set_config", JSON.stringify(config));
        } catch (e) {
          console.warn(`[extism] ${manifest.id}: set_config failed:`, e);
        }
      });
    },

    async close(): Promise<void> {
      try {
        await plugin.call("shutdown", "{}");
      } catch {
        // Optional lifecycle export.
      }
      await transport.dispose();
      await plugin.close();
    },
  };

  if (options?.initializeLifecycle !== false) {
    await browserPlugin.callLifecycle(
      "init",
      await buildBrowserPluginInitPayload(String(manifest.id)),
    );
  }

  return browserPlugin;
}

/**
 * Inspect a plugin WASM to read manifest metadata without installing it.
 */
export async function inspectBrowserPlugin(
  wasmBytes: ArrayBuffer,
): Promise<{ manifest: PluginManifest; requestedPermissions?: RequestedPermissionsManifest }> {
  const plugin = await loadBrowserPlugin(wasmBytes, undefined, {
    initializeLifecycle: false,
  });
  try {
    return {
      manifest: plugin.manifest,
      requestedPermissions: plugin.requestedPermissions,
    };
  } finally {
    await plugin.close();
  }
}
