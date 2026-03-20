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

type Env = {
  ASSETS: AssetsBinding;
  API?: ServiceBinding;
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

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Non-API requests → static assets
    if (!url.pathname.startsWith("/api/") && url.pathname !== "/api") {
      return env.ASSETS.fetch(request);
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
