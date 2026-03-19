/**
 * Cloudflare Worker entrypoint for `app.diaryx.org`.
 *
 * Static assets are served from Workers Static Assets, while `/api/*`
 * requests are proxied to the current Rust sync origin during the migration.
 */

type AssetsBinding = {
  fetch(request: Request): Promise<Response>;
};

type Env = {
  ASSETS: AssetsBinding;
  SYNC_SERVER_ORIGIN?: string;
};

const DEFAULT_SYNC_SERVER_ORIGIN = "https://sync.diaryx.org";

function resolveSyncServerOrigin(env: Env): string {
  const configured = env.SYNC_SERVER_ORIGIN?.trim();
  return configured && configured.length > 0
    ? configured
    : DEFAULT_SYNC_SERVER_ORIGIN;
}

function rewriteApiPath(url: URL): string {
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

    if (!url.pathname.startsWith("/api/")) {
      return env.ASSETS.fetch(request);
    }

    const upstreamPath = rewriteApiPath(url);
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
  },
};
