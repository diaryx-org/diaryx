/**
 * Cloudflare Worker entrypoint for `app.diaryx.org`.
 *
 * Static assets are served from Workers Static Assets.
 * `/api/*` requests are routed to the Rust API worker via service binding,
 * except sync endpoints which are proxied to the native sync server.
 */

type AssetsBinding = {
  fetch(request: Request): Promise<Response>;
};

type ServiceBinding = {
  fetch(request: Request): Promise<Response>;
};

type R2Bucket = {
  get(key: string): Promise<R2Object | null>;
};

type R2Object = {
  body: ReadableStream;
  httpMetadata?: { contentType?: string };
};

type Env = {
  ASSETS: AssetsBinding;
  API?: ServiceBinding;
  CDN?: R2Bucket;
  SYNC_SERVER_ORIGIN?: string;
};

const DEFAULT_SYNC_SERVER_ORIGIN = "https://sync.diaryx.org";

function resolveSyncServerOrigin(env: Env): string {
  const configured = env.SYNC_SERVER_ORIGIN?.trim();
  return configured && configured.length > 0
    ? configured
    : DEFAULT_SYNC_SERVER_ORIGIN;
}

/** Paths that must be proxied to the native sync server (not the Rust API worker). */
function isSyncPath(pathname: string): boolean {
  const stripped = pathname.replace(/^\/api/, "") || "/";

  // WebSocket sync endpoints
  if (/^\/ns\/[^/]+\/sync\/?$/.test(stripped)) return true;
  if (/^\/namespaces\/[^/]+\/sync\/?$/.test(stripped)) return true;
  if (/^\/sync2\/?$/.test(stripped)) return true;

  return false;
}

function rewriteSyncPath(url: URL): string {
  const stripped = url.pathname.replace(/^\/api/, "") || "/";

  const namespaceSyncMatch = stripped.match(/^\/ns\/([^/]+)\/sync\/?$/);
  if (namespaceSyncMatch) {
    const namespaceId = decodeURIComponent(namespaceSyncMatch[1]);
    return `/namespaces/${encodeURIComponent(namespaceId)}/sync`;
  }

  if (/^\/sync2\/?$/.test(stripped)) {
    const workspaceId = url.searchParams.get("workspace_id")?.trim();
    if (workspaceId) {
      return `/namespaces/${encodeURIComponent(workspaceId)}/sync`;
    }
  }

  return stripped;
}

function buildUpstreamRequest(
  request: Request,
  upstreamUrl: URL,
): Request {
  const headers = new Headers(request.headers);
  const sourceUrl = new URL(request.url);
  headers.set("X-Forwarded-Host", sourceUrl.host);
  headers.set("X-Forwarded-Proto", sourceUrl.protocol.replace(":", ""));

  const init: RequestInit = {
    method: request.method,
    headers,
    redirect: "manual",
  };

  if (request.method !== "GET" && request.method !== "HEAD") {
    init.body = request.body;
  }

  return new Request(upstreamUrl.toString(), init);
}

const MIME_TYPES: Record<string, string> = {
  ".md": "text/markdown",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".js": "application/javascript",
  ".css": "text/css",
  ".html": "text/html",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
};

function guessMimeType(key: string): string {
  const ext = key.substring(key.lastIndexOf(".")).toLowerCase();
  return MIME_TYPES[ext] || "application/octet-stream";
}

async function handleCdn(url: URL, env: Env): Promise<Response> {
  if (!env.CDN) {
    return new Response("CDN not configured", { status: 503 });
  }

  // Strip /cdn/ prefix to get the R2 key
  const key = url.pathname.slice("/cdn/".length);
  if (!key) {
    return new Response("Not found", { status: 404 });
  }

  const object = await env.CDN.get(key);
  if (!object) {
    return new Response("Not found", { status: 404 });
  }

  const contentType =
    object.httpMetadata?.contentType || guessMimeType(key);

  return new Response(object.body, {
    headers: {
      "content-type": contentType,
      "cache-control": "public, max-age=3600",
      "access-control-allow-origin": "*",
      "cross-origin-resource-policy": "cross-origin",
    },
  });
}

function handleCdnCors(): Response {
  return new Response(null, {
    status: 204,
    headers: {
      "access-control-allow-origin": "*",
      "access-control-allow-methods": "GET, HEAD, OPTIONS",
      "access-control-max-age": "86400",
    },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Non-API, non-CDN requests → static assets
    if (
      !url.pathname.startsWith("/api/") &&
      url.pathname !== "/api" &&
      !url.pathname.startsWith("/cdn/")
    ) {
      return env.ASSETS.fetch(request);
    }

    // CDN requests → serve from R2 bucket (public, CORS-open)
    if (url.pathname.startsWith("/cdn/")) {
      if (request.method === "OPTIONS") return handleCdnCors();
      return handleCdn(url, env);
    }

    // Sync endpoints → proxy to native sync server
    if (isSyncPath(url.pathname)) {
      const upstreamPath = rewriteSyncPath(url);
      const upstreamUrl = new URL(
        upstreamPath + url.search,
        resolveSyncServerOrigin(env),
      );
      const upstreamRequest = buildUpstreamRequest(request, upstreamUrl);
      const response = await fetch(upstreamRequest);
      return new Response(response.body, {
        status: response.status,
        statusText: response.statusText,
        headers: response.headers,
      });
    }

    // All other API requests → Rust API worker via service binding,
    // or fall back to sync server proxy if the binding isn't configured yet.
    if (env.API) {
      return env.API.fetch(request);
    }

    // Fallback: proxy to native sync server (strips /api prefix)
    const stripped = url.pathname.replace(/^\/api/, "") || "/";
    const upstreamUrl = new URL(
      stripped + url.search,
      resolveSyncServerOrigin(env),
    );
    const upstreamRequest = buildUpstreamRequest(request, upstreamUrl);
    const response = await fetch(upstreamRequest);
    return new Response(response.body, {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
    });
  },
};
