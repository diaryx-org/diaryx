/**
 * Native HTTP proxy for iOS CORS bypass.
 *
 * On iOS, WKWebView enforces CORS and blocks requests from tauri://localhost
 * to external origins like sync.diaryx.org. This module routes HTTP requests
 * through a Tauri command that uses reqwest natively, bypassing CORS.
 *
 * In browser environments, delegates to the native fetch() API.
 */

import { isTauri } from "./interface";

interface ProxyFetchResponse {
  status: number;
  status_text: string;
  headers: Record<string, string>;
  body_base64: string;
}

/** Extended RequestInit with timeout_ms for health checks. */
export interface ProxyFetchInit extends RequestInit {
  /** Timeout in milliseconds (passed to reqwest; AbortSignal can't cross IPC). */
  timeout_ms?: number;
}

/**
 * Drop-in fetch() replacement that routes through native HTTP on Tauri.
 * Returns a real Response object so callers don't need to change their code.
 */
export async function proxyFetch(
  input: RequestInfo | URL,
  init?: ProxyFetchInit,
): Promise<Response> {
  if (!isTauri()) {
    return fetch(input, init);
  }

  const { invoke } = await import("@tauri-apps/api/core");

  // Resolve URL
  const url =
    input instanceof Request
      ? input.url
      : input instanceof URL
        ? input.href
        : input;

  // Resolve method
  const method = init?.method ?? (input instanceof Request ? input.method : "GET");

  // Flatten headers to Record<string, string>
  const headers: Record<string, string> = {};
  const rawHeaders =
    init?.headers ?? (input instanceof Request ? input.headers : undefined);
  if (rawHeaders) {
    if (rawHeaders instanceof Headers) {
      rawHeaders.forEach((v, k) => {
        headers[k] = v;
      });
    } else if (Array.isArray(rawHeaders)) {
      for (const [k, v] of rawHeaders) {
        headers[k] = v;
      }
    } else {
      Object.assign(headers, rawHeaders);
    }
  }

  // Encode body to base64
  let bodyBase64: string | null = null;
  const rawBody = init?.body ?? (input instanceof Request ? input.body : null);
  if (rawBody != null) {
    let bytes: Uint8Array;
    if (typeof rawBody === "string") {
      bytes = new TextEncoder().encode(rawBody);
    } else if (rawBody instanceof ArrayBuffer) {
      bytes = new Uint8Array(rawBody);
    } else if (rawBody instanceof Uint8Array) {
      bytes = rawBody;
    } else if (rawBody instanceof Blob) {
      bytes = new Uint8Array(await rawBody.arrayBuffer());
    } else if (rawBody instanceof ReadableStream) {
      // Read the entire stream
      const reader = rawBody.getReader();
      const chunks: Uint8Array[] = [];
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
      }
      const totalLength = chunks.reduce((sum, c) => sum + c.length, 0);
      bytes = new Uint8Array(totalLength);
      let offset = 0;
      for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.length;
      }
    } else {
      // FormData or URLSearchParams — serialize to string
      bytes = new TextEncoder().encode(String(rawBody));
    }
    bodyBase64 = btoa(String.fromCharCode(...bytes));
  }

  const result = await invoke<ProxyFetchResponse>("proxy_fetch", {
    url,
    method: method.toUpperCase(),
    headers,
    bodyBase64,
    timeoutMs: init?.timeout_ms ?? null,
  });

  // Decode base64 body back to bytes
  const binaryString = atob(result.body_base64);
  const bodyBytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bodyBytes[i] = binaryString.charCodeAt(i);
  }

  // Construct a real Response object
  return new Response(bodyBytes, {
    status: result.status,
    statusText: result.status_text,
    headers: new Headers(result.headers),
  });
}
