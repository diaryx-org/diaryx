/**
 * wasmAuthService — TypeScript wrapper around the wasm-bindgen `AuthClient`,
 * routed through the backend Web Worker.
 *
 * The WASM module is only instantiated inside the worker (see
 * `wasmWorkerNew.ts`). Calling `AuthClient` methods on the main thread would
 * require a second, separately-initialized WASM instance — which previously
 * caused the production `undefined is not an object (_.__wbindgen_malloc)`
 * crash because `wasm.default(...)` was never invoked on the main thread.
 *
 * So instead, this service:
 *
 * - Gets the Comlink-wrapped worker remote via `WorkerBackendNew.getWorkerApi()`.
 * - Sends the HTTP + localStorage callbacks to the worker as `Comlink.proxy`'d
 *   functions so the raw session cookie and localStorage keys stay on the main
 *   thread.
 * - Forwards every AuthClient method to the worker as `remote.authXxx(...)`.
 *
 * Callback responsibilities remain unchanged:
 * - `fetch` issues each request through `proxyFetch` with `credentials: 'include'`
 *   so the browser's HttpOnly session cookie is attached automatically.
 * - `loadMetadata` / `saveMetadata` persist the non-secret `{email, workspace_id}`
 *   pair to localStorage.
 * - `hasSession` / `storeSessionToken` / `clearSession` mirror a boolean flag in
 *   localStorage so the core service knows whether a session exists (the real
 *   cookie is invisible to JS).
 */

import * as Comlink from "comlink";
import { proxyFetch } from "$lib/backend/proxyFetch";
import { getBackend } from "$lib/backend";
import type { WorkerBackendNew } from "$lib/backend/workerBackendNew";
import type { WorkerApi } from "$lib/backend/wasmWorkerNew";
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
// Worker remote wiring — the wasm AuthClient lives in the backend worker,
// so we just keep track of which (remote, serverUrl) pair has been set up
// so we don't pay the round-trip to reinstall callbacks on every call.
// ============================================================================

type WorkerRemote = Comlink.Remote<WorkerApi>;

let cached: {
  remote: WorkerRemote;
  serverUrl: string;
  setupPromise: Promise<void>;
} | null = null;

async function getWorkerRemote(): Promise<WorkerRemote> {
  const backend = await getBackend();
  const maybeWorkerBackend = backend as unknown as Partial<WorkerBackendNew>;
  const getWorkerApi = maybeWorkerBackend.getWorkerApi?.bind(backend);
  const remote = getWorkerApi ? getWorkerApi() : null;
  if (!remote) {
    throw new AuthError(
      "Browser backend is not using the WASM worker — auth is unavailable.",
      0,
    );
  }
  return remote;
}

async function ensureClient(serverUrl: string): Promise<WorkerRemote> {
  const remote = await getWorkerRemote();
  const normalized = serverUrl.replace(/\/+$/, "");

  if (cached?.remote === remote && cached.serverUrl === normalized) {
    await cached.setupPromise;
    return remote;
  }

  const callbacks = Comlink.proxy(makeCallbacks(normalized));
  const entry: {
    remote: WorkerRemote;
    serverUrl: string;
    setupPromise: Promise<void>;
  } = {
    remote,
    serverUrl: normalized,
    setupPromise: undefined as unknown as Promise<void>,
  };
  entry.setupPromise = remote
    .authSetServerUrl(normalized, callbacks as unknown as unknown)
    .catch((e: unknown) => {
      // Invalidate the cache on setup failure so the next call retries.
      if (cached === entry) cached = null;
      throw e;
    });
  cached = entry;
  await entry.setupPromise;
  return remote;
}

/**
 * Reset the cached wasm auth client. Call this when the server URL changes
 * (e.g. the user switches sync servers) so the next request uses the new URL.
 */
export function resetWasmAuthClient(): void {
  const prev = cached;
  cached = null;
  if (!prev) return;
  // Fire-and-forget: tell the worker to drop its AuthClient too. We wait on
  // the setup promise first so we don't race with an in-flight install.
  Promise.resolve(prev.setupPromise)
    .catch(() => {
      /* install failure already propagated to the original caller */
    })
    .then(() => prev.remote.authReset())
    .catch(() => {
      /* best-effort */
    });
}

// ============================================================================
// CoreAuthService implementation
// ============================================================================

/**
 * Create a `CoreAuthService` backed by the wasm `AuthClient` running inside
 * the backend worker.
 *
 * @param getServerUrl — invoked lazily on each call so the router can pick up
 * live changes to the user-configured server URL without reconstructing.
 */
export function createWasmAuthService(
  getServerUrl: () => string | null,
): CoreAuthService {
  async function remote(): Promise<WorkerRemote> {
    const url = getServerUrl();
    if (!url) {
      throw new AuthError("Server URL not configured", 0);
    }
    return ensureClient(url);
  }

  return {
    async isAuthenticated() {
      try {
        const r = await remote();
        return await r.authIsAuthenticated();
      } catch {
        return false;
      }
    },

    async getMetadata() {
      try {
        const r = await remote();
        return parseNullableJson<CoreAuthMetadata>(await r.authGetMetadata());
      } catch {
        return null;
      }
    },

    async requestMagicLink(email) {
      try {
        const r = await remote();
        return parseJson<MagicLinkResponse>(
          await r.authRequestMagicLink(email),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async verifyMagicLink(token, deviceName, replaceDeviceId) {
      try {
        const r = await remote();
        return parseJson<VerifyResponse>(
          await r.authVerifyMagicLink(
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
        const r = await remote();
        return parseJson<VerifyResponse>(
          await r.authVerifyCode(
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
        const r = await remote();
        return parseJson<MeResponse>(await r.authGetMe());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async refreshToken() {
      try {
        const r = await remote();
        return parseJson<MeResponse>(await r.authRefreshToken());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async logout() {
      try {
        const r = await remote();
        await r.authLogout();
      } catch (err) {
        throw wrapError(err);
      }
    },

    async getDevices() {
      try {
        const r = await remote();
        return parseJson<Device[]>(await r.authGetDevices());
      } catch (err) {
        throw wrapError(err);
      }
    },

    async renameDevice(deviceId, newName) {
      try {
        const r = await remote();
        await r.authRenameDevice(deviceId, newName);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteDevice(deviceId) {
      try {
        const r = await remote();
        await r.authDeleteDevice(deviceId);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteAccount() {
      try {
        const r = await remote();
        await r.authDeleteAccount();
      } catch (err) {
        throw wrapError(err);
      }
    },

    async createWorkspace(name) {
      try {
        const r = await remote();
        return parseJson<Workspace>(await r.authCreateWorkspace(name));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async renameWorkspace(workspaceId, newName) {
      try {
        const r = await remote();
        await r.authRenameWorkspace(workspaceId, newName);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteWorkspace(workspaceId) {
      try {
        const r = await remote();
        await r.authDeleteWorkspace(workspaceId);
      } catch (err) {
        throw wrapError(err);
      }
    },
  };
}
