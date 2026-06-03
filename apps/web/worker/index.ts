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
};

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

    // API requests → Rust API worker via service binding
    if (env.API) {
      return env.API.fetch(request);
    }

    return new Response("API service binding is not configured", { status: 503 });
  },
};
