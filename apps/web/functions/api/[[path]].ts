/**
 * Cloudflare Pages Function — reverse proxy for /api/* → sync server VPS.
 *
 * This makes the sync server same-origin with the web app, enabling
 * HttpOnly cookies with SameSite=Strict for secure session management.
 */

const SYNC_SERVER_ORIGIN = "https://sync.diaryx.org";

export const onRequest: PagesFunction = async (context) => {
  const url = new URL(context.request.url);

  // Strip the /api prefix to get the path the sync server expects
  const upstreamPath = url.pathname.replace(/^\/api/, "") || "/";
  const upstream = new URL(upstreamPath + url.search, SYNC_SERVER_ORIGIN);

  // Clone headers, forwarding the original Host as X-Forwarded-Host
  const headers = new Headers(context.request.headers);
  headers.set("X-Forwarded-Host", url.host);
  headers.set("X-Forwarded-Proto", url.protocol.replace(":", ""));

  // Proxy the request
  const response = await fetch(upstream.toString(), {
    method: context.request.method,
    headers,
    body: context.request.body,
    redirect: "manual",
  });

  // Return the response, passing through all headers (including Set-Cookie)
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: response.headers,
  });
};
