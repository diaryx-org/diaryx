/**
 * publishProvider — the main-thread `NamespaceProvider` implementation for the
 * browser publish path.
 *
 * The core publish algorithm runs inside the WASM backend worker (it owns the
 * workspace filesystem); the actual namespace HTTP calls run here on the main
 * thread (where the session cookie is sent automatically via
 * `credentials: 'include'`). The object returned by `createPublishProvider` is
 * `Comlink.proxy`'d into the worker, where Rust's `JsNamespaceProvider` forwards
 * each `NamespaceProvider` method to the matching function below.
 *
 * Mirrors the native (Tauri) `KeyringAuthenticatedClient` NamespaceProvider impl
 * — same routes, same ARK headers — so both platforms upload identically.
 */

const NAMESPACE_REQUEST_TIMEOUT_MS = 60_000;

/** Percent-encode a key, preserving `/` separators (matches the native client). */
function encodeKeyPath(key: string): string {
  return key
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

export interface PublishProvider {
  listObjects(nsId: string): Promise<string>;
  putObject(
    nsId: string,
    key: string,
    bytes: Uint8Array,
    mimeType: string,
    audience: string | null,
    fileArk: string | null,
    sourceKey: string | null,
    objectKey: string | null,
    isIndex: boolean,
  ): Promise<void>;
  deleteObject(nsId: string, key: string): Promise<void>;
  syncAudience(nsId: string, audience: string, gatesJson: string): Promise<void>;
  listAudiences(nsId: string): Promise<string>;
  deleteAudience(nsId: string, audience: string): Promise<void>;
  buildNamespace(nsId: string, baseUrl: string | null): Promise<void>;
}

/**
 * Build a `PublishProvider` bound to `serverUrl`. Each method performs a
 * same-origin authenticated fetch (cookie via `credentials: 'include'`).
 */
export function createPublishProvider(serverUrl: string): PublishProvider {
  const base = serverUrl.replace(/\/+$/, "");

  async function send(
    method: string,
    path: string,
    init: Omit<RequestInit, "method" | "credentials"> = {},
    okStatuses: number[] = [],
  ): Promise<Uint8Array> {
    const controller =
      typeof AbortController === "function" ? new AbortController() : null;
    const timeoutId = controller
      ? globalThis.setTimeout(
          () => controller.abort(),
          NAMESPACE_REQUEST_TIMEOUT_MS,
        )
      : null;
    try {
      const response = await fetch(`${base}${path}`, {
        ...init,
        method,
        credentials: "include",
        signal: controller?.signal ?? init.signal,
      });
      const bytes = new Uint8Array(await response.arrayBuffer());
      if (!response.ok && !okStatuses.includes(response.status)) {
        let text = "";
        try {
          text = new TextDecoder().decode(bytes);
        } catch {
          text = "";
        }
        throw new Error(text || `${method} ${path} returned ${response.status}`);
      }
      return bytes;
    } catch (e) {
      if (e instanceof Error && e.name === "AbortError") {
        throw new Error(
          `Request timed out after ${NAMESPACE_REQUEST_TIMEOUT_MS}ms`,
        );
      }
      throw e;
    } finally {
      if (timeoutId !== null) globalThis.clearTimeout(timeoutId);
    }
  }

  async function sendText(
    method: string,
    path: string,
    init?: Omit<RequestInit, "method" | "credentials">,
    okStatuses?: number[],
  ): Promise<string> {
    const bytes = await send(method, path, init, okStatuses);
    return new TextDecoder().decode(bytes);
  }

  return {
    async listObjects(nsId) {
      // Returned verbatim; Rust decodes the JSON array of { key, audience, content_hash }.
      return sendText(
        "GET",
        `/namespaces/${encodeURIComponent(nsId)}/objects`,
      );
    },

    async putObject(
      nsId,
      key,
      bytes,
      mimeType,
      audience,
      fileArk,
      sourceKey,
      objectKey,
      isIndex,
    ) {
      const headers: Record<string, string> = { "Content-Type": mimeType };
      if (audience) headers["X-Audience"] = audience;
      if (fileArk) headers["X-Diaryx-File-Ark"] = fileArk;
      if (sourceKey) headers["X-Diaryx-Source-Key"] = sourceKey;
      if (objectKey) headers["X-Diaryx-Object-Key"] = objectKey;
      if (isIndex) headers["X-Diaryx-Is-Index"] = "true";
      await send(
        "PUT",
        `/namespaces/${encodeURIComponent(nsId)}/objects/${encodeKeyPath(key)}`,
        { headers, body: bytes as unknown as BodyInit },
      );
    },

    async deleteObject(nsId, key) {
      await send(
        "DELETE",
        `/namespaces/${encodeURIComponent(nsId)}/objects/${encodeKeyPath(key)}`,
        {},
        [404],
      );
    },

    async syncAudience(nsId, audience, gatesJson) {
      await send(
        "PUT",
        `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audience)}`,
        {
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ gates: JSON.parse(gatesJson) }),
        },
      );
    },

    async listAudiences(nsId) {
      const text = await sendText(
        "GET",
        `/namespaces/${encodeURIComponent(nsId)}/audiences`,
      );
      // Server returns `[{ namespace_id, name, gates }]`; Rust expects a JSON
      // array of audience-name strings.
      const items = JSON.parse(text) as Array<{ name: string }>;
      return JSON.stringify(items.map((a) => a.name));
    },

    async deleteAudience(nsId, audience) {
      await send(
        "DELETE",
        `/namespaces/${encodeURIComponent(nsId)}/audiences/${encodeURIComponent(audience)}`,
        {},
        [404],
      );
    },

    async buildNamespace(nsId, baseUrl) {
      const query = baseUrl ? `?base_url=${encodeURIComponent(baseUrl)}` : "";
      await send("POST", `/namespaces/${encodeURIComponent(nsId)}/build${query}`);
    },
  };
}
