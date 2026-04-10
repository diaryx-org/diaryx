/**
 * wasmAuthService — TypeScript wrapper around the wasm-bindgen `AuthClient`.
 *
 * Runs diaryx_core's `AuthService` inside WASM and delegates HTTP and
 * credential persistence back to JS via the `AuthCallbacks` interface:
 *
 * - `fetch` issues each request through `proxyFetch` with
 *   `credentials: 'include'` so the browser's HttpOnly session cookie is
 *   attached automatically. The raw session token never touches JS.
 * - `loadMetadata` / `saveMetadata` persist the non-secret `{email,
 *   workspace_id}` pair to localStorage.
 * - `hasSession` / `storeSessionToken` / `clearSession` mirror a boolean flag
 *   in localStorage so the core service knows whether a session exists (the
 *   real cookie is invisible to JS).
 *
 * The module exposes a lazy singleton keyed by server URL and a narrow
 * `CoreAuthService` interface that tauriAuthService also implements so the
 * two can be swapped behind an `isTauri()` router.
 */

import { proxyFetch } from "$lib/backend/proxyFetch";
import type {
  CoreAuthService,
  CoreAuthMetadata,
  MagicLinkResponse,
  VerifyResponse,
  MeResponse,
  Device,
  Workspace,
} from "./coreAuthTypes";
import { AuthError } from "./coreAuthTypes";

// localStorage keys — private to this module.
const META_KEY = "diaryx_auth_metadata";
const HAS_SESSION_KEY = "diaryx_has_session";

type WasmAuthClientCtor = new (serverUrl: string, callbacks: unknown) => {
  readonly serverUrl: string;
  isAuthenticated(): Promise<boolean>;
  getMetadata(): Promise<unknown>;
  requestMagicLink(email: string): Promise<unknown>;
  verifyMagicLink(
    token: string,
    deviceName?: string | null,
    replaceDeviceId?: string | null,
  ): Promise<unknown>;
  verifyCode(
    code: string,
    email: string,
    deviceName?: string | null,
    replaceDeviceId?: string | null,
  ): Promise<unknown>;
  getMe(): Promise<unknown>;
  refreshToken(): Promise<unknown>;
  logout(): Promise<void>;
  getDevices(): Promise<unknown>;
  renameDevice(deviceId: string, newName: string): Promise<void>;
  deleteDevice(deviceId: string): Promise<void>;
  deleteAccount(): Promise<void>;
  createWorkspace(name: string): Promise<unknown>;
  renameWorkspace(workspaceId: string, newName: string): Promise<void>;
  deleteWorkspace(workspaceId: string): Promise<void>;
};

interface AuthCallbacks {
  fetch(
    method: string,
    path: string,
    body: string | null,
  ): Promise<{ status: number; body: string }>;
  loadMetadata(): Promise<CoreAuthMetadata | null>;
  saveMetadata(metadata: CoreAuthMetadata): Promise<void>;
  hasSession(): Promise<boolean>;
  storeSessionToken(token: string): Promise<void>;
  clearSession(): Promise<void>;
}

/**
 * Build the callback object that the wasm AuthClient calls back into.
 */
function makeCallbacks(serverUrl: string): AuthCallbacks {
  const base = serverUrl.replace(/\/+$/, "");
  return {
    fetch: async (method, path, body) => {
      const url = path.startsWith("http://") || path.startsWith("https://")
        ? path
        : `${base}${path.startsWith("/") ? path : `/${path}`}`;

      const headers: Record<string, string> = {};
      if (body !== null && body !== undefined) {
        headers["Content-Type"] = "application/json";
      }

      const resp = await proxyFetch(url, {
        method,
        headers,
        body: body ?? undefined,
        // proxyFetch preserves credentials: 'include' for browser fetches;
        // on Tauri this call path is unused (tauriAuthService handles IPC).
      });

      // Drain to text so the wasm side can `HttpResponse::json::<T>()`.
      const text = await resp.text();
      return { status: resp.status, body: text };
    },

    loadMetadata: async () => {
      if (typeof localStorage === "undefined") return null;
      const raw = localStorage.getItem(META_KEY);
      if (!raw) return null;
      try {
        return JSON.parse(raw) as CoreAuthMetadata;
      } catch {
        return null;
      }
    },

    saveMetadata: async (metadata) => {
      if (typeof localStorage === "undefined") return;
      localStorage.setItem(META_KEY, JSON.stringify(metadata));
    },

    hasSession: async () => {
      if (typeof localStorage === "undefined") return false;
      return localStorage.getItem(HAS_SESSION_KEY) === "true";
    },

    storeSessionToken: async (_token) => {
      // Browser: the real token lives in an HttpOnly cookie set by the
      // server. We only mirror a boolean flag so the wasm client can answer
      // `hasSession()`.
      if (typeof localStorage === "undefined") return;
      localStorage.setItem(HAS_SESSION_KEY, "true");
    },

    clearSession: async () => {
      if (typeof localStorage === "undefined") return;
      localStorage.removeItem(HAS_SESSION_KEY);
    },
  };
}

/** Parse a wasm return value that is either a JSON string or null. */
function parseJson<T>(value: unknown): T {
  if (typeof value === "string") {
    return JSON.parse(value) as T;
  }
  // Defensive: wasm-bindgen has been known to hand back already-decoded
  // JsValues for pass-through returns. Accept both shapes.
  return value as T;
}

function parseNullableJson<T>(value: unknown): T | null {
  if (value === null || value === undefined) return null;
  return parseJson<T>(value);
}

/**
 * Convert a thrown wasm error into an `AuthError` with `statusCode` and
 * optional `devices` preserved as set by the Rust side.
 */
function wrapError(err: unknown): AuthError {
  if (err instanceof AuthError) return err;
  if (err instanceof Error) {
    const anyErr = err as Error & {
      statusCode?: number;
      devices?: Device[];
    };
    return new AuthError(
      anyErr.message || "Auth request failed",
      anyErr.statusCode ?? 0,
      undefined,
      anyErr.devices,
    );
  }
  return new AuthError(
    typeof err === "string" ? err : "Auth request failed",
    0,
  );
}

// ============================================================================
// Singleton wiring — the wasm AuthClient is lazy-constructed per server URL
// ============================================================================

let cached: {
  serverUrl: string;
  client: InstanceType<WasmAuthClientCtor>;
} | null = null;

async function loadAuthClientClass(): Promise<WasmAuthClientCtor> {
  const wasm = await import("$wasm");
  return (wasm as unknown as { AuthClient: WasmAuthClientCtor }).AuthClient;
}

async function getClient(
  serverUrl: string,
): Promise<InstanceType<WasmAuthClientCtor>> {
  const normalized = serverUrl.replace(/\/+$/, "");
  if (cached && cached.serverUrl === normalized) {
    return cached.client;
  }
  const AuthClientCtor = await loadAuthClientClass();
  const client = new AuthClientCtor(
    normalized,
    makeCallbacks(normalized),
  ) as InstanceType<WasmAuthClientCtor>;
  cached = { serverUrl: normalized, client };
  return client;
}

/**
 * Reset the cached wasm auth client. Call this when the server URL changes
 * (e.g. the user switches sync servers) so the next request uses the new URL.
 */
export function resetWasmAuthClient(): void {
  cached = null;
}

// ============================================================================
// CoreAuthService implementation
// ============================================================================

/**
 * Create a `CoreAuthService` backed by the wasm `AuthClient`.
 *
 * @param getServerUrl — invoked lazily on each call so the router can pick up
 * live changes to the user-configured server URL without reconstructing.
 */
export function createWasmAuthService(
  getServerUrl: () => string | null,
): CoreAuthService {
  async function client(): Promise<InstanceType<WasmAuthClientCtor>> {
    const url = getServerUrl();
    if (!url) {
      throw new AuthError("Server URL not configured", 0);
    }
    return getClient(url);
  }

  return {
    async isAuthenticated() {
      try {
        const c = await client();
        return await c.isAuthenticated();
      } catch {
        return false;
      }
    },

    async getMetadata() {
      try {
        const c = await client();
        return parseNullableJson<CoreAuthMetadata>(await c.getMetadata());
      } catch {
        return null;
      }
    },

    async requestMagicLink(email) {
      try {
        const c = await client();
        return parseJson<MagicLinkResponse>(await c.requestMagicLink(email));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async verifyMagicLink(token, deviceName, replaceDeviceId) {
      try {
        const c = await client();
        return parseJson<VerifyResponse>(
          await c.verifyMagicLink(
            token,
            deviceName ?? null,
            replaceDeviceId ?? null,
          ),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async verifyCode(code, email, deviceName, replaceDeviceId) {
      try {
        const c = await client();
        return parseJson<VerifyResponse>(
          await c.verifyCode(
            code,
            email,
            deviceName ?? null,
            replaceDeviceId ?? null,
          ),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async getMe() {
      try {
        const c = await client();
        return parseJson<MeResponse>(await c.getMe());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async refreshToken() {
      try {
        const c = await client();
        return parseJson<MeResponse>(await c.refreshToken());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async logout() {
      try {
        const c = await client();
        await c.logout();
      } catch (err) {
        throw wrapError(err);
      }
    },

    async getDevices() {
      try {
        const c = await client();
        return parseJson<Device[]>(await c.getDevices());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async renameDevice(deviceId, newName) {
      try {
        const c = await client();
        await c.renameDevice(deviceId, newName);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteDevice(deviceId) {
      try {
        const c = await client();
        await c.deleteDevice(deviceId);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteAccount() {
      try {
        const c = await client();
        await c.deleteAccount();
      } catch (err) {
        throw wrapError(err);
      }
    },

    async createWorkspace(name) {
      try {
        const c = await client();
        return parseJson<Workspace>(await c.createWorkspace(name));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async renameWorkspace(workspaceId, newName) {
      try {
        const c = await client();
        await c.renameWorkspace(workspaceId, newName);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteWorkspace(workspaceId) {
      try {
        const c = await client();
        await c.deleteWorkspace(workspaceId);
      } catch (err) {
        throw wrapError(err);
      }
    },
  };
}
