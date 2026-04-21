/**
 * wasmNamespaceService — TypeScript wrapper around the wasm-bindgen
 * `NamespaceClient`, routed through the backend Web Worker.
 *
 * Mirrors `wasmAuthService.ts`: the WASM module is only instantiated inside
 * the worker, so every namespace method forwards to `remote.namespaceXxx(...)`.
 * HTTP + localStorage callbacks live on the main thread and are sent in via
 * `Comlink.proxy` so the raw session cookie stays here.
 */

import * as Comlink from "comlink";
import { proxyFetch } from "$lib/backend/proxyFetch";
import { getBackend } from "$lib/backend";
import type { WorkerBackendNew } from "$lib/backend/workerBackendNew";
import type { WorkerApi } from "$lib/backend/wasmWorkerNew";
import { AuthError } from "$lib/auth/coreAuthTypes";
import type {
  CoreNamespaceService,
  NamespaceInfo,
  AudienceInfo,
  SubdomainInfo,
  DomainInfo,
  TokenResult,
  SubscriberInfo,
  BulkImportResult,
} from "./coreNamespaceTypes";

// ============================================================================
// HTTP callbacks — identical shape to wasmAuthService, but scoped to this
// module so the NamespaceClient can evolve independently if we ever extract
// it out of the shared AuthCallbacks type.
// ============================================================================

interface NamespaceCallbacks {
  fetch(
    method: string,
    path: string,
    body: string | null,
  ): Promise<{ status: number; body: string }>;
  // The fields below are part of AuthCallbacks but unused by NamespaceClient.
  // Kept as no-ops so the shared Rust trait implementation doesn't panic.
  loadMetadata(): Promise<null>;
  saveMetadata(): Promise<void>;
  hasSession(): Promise<boolean>;
  storeSessionToken(): Promise<void>;
  clearSession(): Promise<void>;
}

function makeCallbacks(serverUrl: string): NamespaceCallbacks {
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
      });
      const text = await resp.text();
      return { status: resp.status, body: text };
    },
    loadMetadata: async () => null,
    saveMetadata: async () => {},
    hasSession: async () => true,
    storeSessionToken: async () => {},
    clearSession: async () => {},
  };
}

// ============================================================================
// Worker remote + response parsing
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
      "Browser backend is not using the WASM worker — namespace API is unavailable.",
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
    .namespaceSetServerUrl(normalized, callbacks as unknown as unknown)
    .catch((e: unknown) => {
      if (cached === entry) cached = null;
      throw e;
    });
  cached = entry;
  await entry.setupPromise;
  return remote;
}

/**
 * Reset the cached wasm namespace client. Call this when the server URL
 * changes so the next request uses the new URL.
 */
export function resetWasmNamespaceClient(): void {
  const prev = cached;
  cached = null;
  if (!prev) return;
  Promise.resolve(prev.setupPromise)
    .catch(() => {})
    .then(() => prev.remote.namespaceReset())
    .catch(() => {});
}

function parseJson<T>(value: unknown): T {
  if (typeof value === "string") return JSON.parse(value) as T;
  return value as T;
}

function wrapError(err: unknown): AuthError {
  if (err instanceof AuthError) return err;
  if (err instanceof Error) {
    const anyErr = err as Error & { statusCode?: number };
    return new AuthError(
      anyErr.message || "Namespace request failed",
      anyErr.statusCode ?? 0,
    );
  }
  return new AuthError(
    typeof err === "string" ? err : "Namespace request failed",
    0,
  );
}

// ============================================================================
// CoreNamespaceService implementation
// ============================================================================

/**
 * Create a `CoreNamespaceService` backed by the wasm `NamespaceClient`
 * running inside the backend worker.
 */
export function createWasmNamespaceService(
  getServerUrl: () => string | null,
): CoreNamespaceService {
  async function remote(): Promise<WorkerRemote> {
    const url = getServerUrl();
    if (!url) {
      throw new AuthError("Server URL not configured", 0);
    }
    return ensureClient(url);
  }

  return {
    async getNamespace(id) {
      try {
        const r = await remote();
        return parseJson<NamespaceInfo>(await r.namespaceGet(id));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async createNamespace(id, metadata) {
      try {
        const r = await remote();
        return parseJson<NamespaceInfo>(
          await r.namespaceCreate(id ?? null, metadata ?? null),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async updateNamespaceMetadata(id, metadata) {
      try {
        const r = await remote();
        return parseJson<NamespaceInfo>(
          await r.namespaceUpdateMetadata(id, metadata),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async deleteNamespace(id) {
      try {
        const r = await remote();
        await r.namespaceDelete(id);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async listAudiences(id) {
      try {
        const r = await remote();
        return parseJson<AudienceInfo[]>(await r.namespaceListAudiences(id));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async setAudience(id, name, access) {
      try {
        const r = await remote();
        await r.namespaceSetAudience(id, name, access);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async getAudienceToken(id, name) {
      try {
        const r = await remote();
        return parseJson<TokenResult>(await r.namespaceGetAudienceToken(id, name));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async claimSubdomain(id, subdomain, defaultAudience) {
      try {
        const r = await remote();
        return parseJson<SubdomainInfo>(
          await r.namespaceClaimSubdomain(
            id,
            subdomain,
            defaultAudience ?? null,
          ),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async releaseSubdomain(id) {
      try {
        const r = await remote();
        await r.namespaceReleaseSubdomain(id);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async listDomains(id) {
      try {
        const r = await remote();
        return parseJson<DomainInfo[]>(await r.namespaceListDomains(id));
      } catch (err) {
        throw wrapError(err);
      }
    },

    async registerDomain(id, domain, audienceName) {
      try {
        const r = await remote();
        return parseJson<DomainInfo>(
          await r.namespaceRegisterDomain(id, domain, audienceName),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async removeDomain(id, domain) {
      try {
        const r = await remote();
        await r.namespaceRemoveDomain(id, domain);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async listSubscribers(id, audience) {
      try {
        const r = await remote();
        return parseJson<SubscriberInfo[]>(
          await r.namespaceListSubscribers(id, audience),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async addSubscriber(id, audience, email) {
      try {
        const r = await remote();
        return parseJson<SubscriberInfo>(
          await r.namespaceAddSubscriber(id, audience, email),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },

    async removeSubscriber(id, audience, contactId) {
      try {
        const r = await remote();
        await r.namespaceRemoveSubscriber(id, audience, contactId);
      } catch (err) {
        throw wrapError(err);
      }
    },

    async bulkImportSubscribers(id, audience, emails) {
      try {
        const r = await remote();
        return parseJson<BulkImportResult>(
          await r.namespaceBulkImportSubscribers(id, audience, emails),
        );
      } catch (err) {
        throw wrapError(err);
      }
    },
  };
}
