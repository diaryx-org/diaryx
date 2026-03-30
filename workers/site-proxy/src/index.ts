import { TokenClaims, validateSignedToken } from './token';

type Env = {
  SITES_BUCKET: R2Bucket;
  ATTACHMENTS_BUCKET: R2Bucket;
  TOKEN_SIGNING_KEY: string;
  KV: KVNamespace;
  DB: D1Database;
};

type SiteMeta = {
  user_id: string;
  audiences: string[];
  revoked_tokens: string[];
  attachment_prefix: string;
  public_audiences?: string[];
};

type AuthResult = {
  audience: string;
  earlyResponse?: Response;
};

// KV domain mapping written by the sync server.
type DomainMapping = { namespace_id: string; audience_name: string };

// KV subdomain mapping: subdomain → namespace + optional default audience.
type SubdomainMapping = { namespace_id: string; default_audience?: string };

type NamespaceObjectMeta = {
  key: string;
  r2_key: string | null;
  mime_type: string;
  audience: string | null;
};

type NamespaceAudienceMeta = {
  access: string;
};

const META_TTL_MS = 60_000;
const metaCache = new Map<string, { expiresAt: number; value: SiteMeta | null }>();

const SITE_DOMAIN = 'diaryx.org';
const ROOT_SITE_SUBDOMAIN = 'main';

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Subdomain routing: {name}.diaryx.org → namespace
    // For infrastructure subdomains (app, sync, etc.), pass through to origin.
    const host = url.hostname;
    if (isPassthroughSubdomain(host)) {
      return fetch(request);
    }
    if (host === SITE_DOMAIN) {
      const rootMapping = await resolveNamedSubdomain(ROOT_SITE_SUBDOMAIN, env);
      if (rootMapping) {
        return serveSubdomainSite(request, env, rootMapping, url);
      }
    }
    const subdomainMapping = await resolveSubdomain(host, env);
    if (subdomainMapping) {
      return serveSubdomainSite(request, env, subdomainMapping, url);
    }

    // Custom domain support — two paths:
    // 1. Cloudflare for SaaS: the Host header IS the custom domain (no proxy header needed).
    // 2. Legacy Caddy proxy: X-Custom-Domain header is set by the upstream proxy.
    // Skip for /ns/ routes — those always use standard namespace routing.
    if (!url.pathname.startsWith('/ns/')) {
      // Try the Host header as a custom domain (Cloudflare for SaaS path).
      // This applies when the host is not *.diaryx.org (subdomainMapping was null above).
      if (!host.endsWith(`.${SITE_DOMAIN}`) && host !== SITE_DOMAIN) {
        const mapping = await resolveNamespaceDomain(host, env);
        if (mapping) {
          return serveNamespaceCustomDomain(request, env, mapping, url);
        }
      }

      // Legacy Caddy proxy path: X-Custom-Domain header.
      const customDomain = request.headers.get('X-Custom-Domain');
      if (customDomain) {
        const mapping = await resolveNamespaceDomain(customDomain, env);
        if (mapping) {
          return serveNamespaceCustomDomain(request, env, mapping, url);
        }

        // Fall back to legacy slug-based custom domain (requires proxy secret).
        const secret = request.headers.get('X-Proxy-Secret');
        if (secret !== env.TOKEN_SIGNING_KEY) {
          return forbidden('Invalid proxy request.');
        }
        return serveCustomDomain(request, env, customDomain, url);
      }
    }

    // Standard routing
    const segments = url.pathname.split('/').filter(Boolean);
    if (segments.length === 0) {
      return notFound();
    }

    const slug = segments[0];

    // Namespace object route: /ns/{ns_id}/{*path}
    if (slug === 'ns') {
      return serveNamespaceRoute(request, env, url, segments);
    }
    if (slug === 'sites') {
      return serveNamespaceSiteRoute(request, env, url, segments);
    }
    const sitePathSegments = segments.slice(1);
    const siteMeta = await getSiteMeta(slug, env);
    if (!siteMeta) {
      return notFound();
    }

    const auth = await authenticateAudience(request, url, env, siteMeta, slug);
    if (auth.earlyResponse) {
      return auth.earlyResponse;
    }

    if (sitePathSegments[0] === '_a') {
      return serveAttachment(sitePathSegments, auth.audience, siteMeta, env);
    }

    // Canonicalize "/{slug}" to "/{slug}/" so relative links inside index.html
    // resolve under the site prefix instead of the origin root.
    if (sitePathSegments.length === 0 && !url.pathname.endsWith('/')) {
      const canonicalUrl = new URL(url.toString());
      canonicalUrl.pathname = `${url.pathname}/`;
      return new Response(null, {
        status: 302,
        headers: {
          Location: canonicalUrl.toString(),
        },
      });
    }

    return serveStaticPage(slug, sitePathSegments, auth.audience, env);
  },
};

async function authenticateAudience(
  request: Request,
  url: URL,
  env: Env,
  siteMeta: SiteMeta,
  slug: string,
): Promise<AuthResult> {
  const access = url.searchParams.get('access');
  if (access) {
    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, access);
    if (!claims || !isClaimsAllowed(claims, siteMeta, slug)) {
      return {
        audience: 'public',
        earlyResponse: forbidden('This access token is invalid or has expired.'),
      };
    }

    const redirectUrl = new URL(url.toString());
    redirectUrl.searchParams.delete('access');
    const headers = new Headers();
    headers.set('Location', redirectUrl.toString());
    headers.append('Set-Cookie', buildCookie(slug, access));
    const response = new Response(null, {
      status: 302,
      headers,
    });
    return {
      audience: claims.a,
      earlyResponse: response,
    };
  }

  const cookieValue = getCookie(request.headers.get('Cookie') ?? '', `diaryx_access_${slug}`);
  if (cookieValue) {
    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, cookieValue);
    if (claims && isClaimsAllowed(claims, siteMeta, slug)) {
      return { audience: claims.a };
    }
  }

  // Use the first public audience, or fall back to 'public' for backwards compat.
  // If public_audiences is explicitly empty, deny unauthenticated visitors.
  const publicAud = siteMeta.public_audiences;
  if (publicAud && publicAud.length === 0) {
    return { audience: 'public', earlyResponse: forbidden('Authentication required to view this site.') };
  }
  return { audience: publicAud?.[0] ?? 'public' };
}

async function serveCustomDomain(
  request: Request,
  env: Env,
  hostname: string,
  url: URL,
): Promise<Response> {
  // Resolve domain → slug via KV.
  const slug = await env.KV.get(`domain:${hostname.toLowerCase()}`);
  if (!slug) {
    return notFound();
  }

  const siteMeta = await getSiteMeta(slug, env);
  if (!siteMeta) {
    return notFound();
  }

  const segments = url.pathname.split('/').filter(Boolean);

  const auth = await authenticateAudienceCustomDomain(request, url, env, siteMeta, slug);
  if (auth.earlyResponse) {
    return auth.earlyResponse;
  }

  // Attachment route: /_a/{audience}/{hash}/{filename}
  if (segments[0] === '_a') {
    return serveAttachment(segments, auth.audience, siteMeta, env);
  }

  // Canonicalize root to "/" with trailing slash.
  if (segments.length === 0 && !url.pathname.endsWith('/')) {
    const canonicalUrl = new URL(url.toString());
    canonicalUrl.pathname = '/';
    return new Response(null, { status: 302, headers: { Location: canonicalUrl.toString() } });
  }

  // Serve static page — same R2 layout, just no slug in the URL.
  let path = segments.map((s) => decodeURIComponent(s)).join('/');
  if (!path) {
    path = 'index.html';
  }

  let key = `${slug}/${auth.audience}/${path}`;
  let object = await env.SITES_BUCKET.get(key);

  if (!object && !path.endsWith('/index.html')) {
    key = `${slug}/${auth.audience}/${path.replace(/\/$/, '')}/index.html`;
    object = await env.SITES_BUCKET.get(key);
  }

  if (!object) {
    return notFound();
  }

  const contentType = object.httpMetadata?.contentType ?? contentTypeForPath(path);
  const headers = new Headers();
  headers.set('Content-Type', contentType);
  headers.set('Cache-Control', 'public, max-age=60');

  if (contentType.toLowerCase().includes('text/html')) {
    const html = await object.text();
    const rewritten = stripSlugPrefix(html, slug);
    return new Response(rewritten, { headers });
  }

  return new Response(object.body, { headers });
}

async function authenticateAudienceCustomDomain(
  request: Request,
  url: URL,
  env: Env,
  siteMeta: SiteMeta,
  slug: string,
): Promise<AuthResult> {
  const access = url.searchParams.get('access');
  if (access) {
    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, access);
    if (!claims || !isClaimsAllowed(claims, siteMeta, slug)) {
      return {
        audience: 'public',
        earlyResponse: forbidden('This access token is invalid or has expired.'),
      };
    }

    const redirectUrl = new URL(url.toString());
    redirectUrl.searchParams.delete('access');
    const headers = new Headers();
    headers.set('Location', redirectUrl.toString());
    headers.append('Set-Cookie', buildCookie(slug, access, '/'));
    return {
      audience: claims.a,
      earlyResponse: new Response(null, { status: 302, headers }),
    };
  }

  const cookieValue = getCookie(request.headers.get('Cookie') ?? '', `diaryx_access_${slug}`);
  if (cookieValue) {
    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, cookieValue);
    if (claims && isClaimsAllowed(claims, siteMeta, slug)) {
      return { audience: claims.a };
    }
  }

  const publicAud = siteMeta.public_audiences;
  if (publicAud && publicAud.length === 0) {
    return { audience: 'public', earlyResponse: forbidden('Authentication required to view this site.') };
  }
  return { audience: publicAud?.[0] ?? 'public' };
}

function isClaimsAllowed(claims: TokenClaims, siteMeta: SiteMeta, slug: string): boolean {
  if (claims.s !== slug) {
    return false;
  }
  if (claims.e !== null && claims.e < Math.floor(Date.now() / 1000)) {
    return false;
  }
  if (siteMeta.revoked_tokens.includes(claims.t)) {
    return false;
  }
  return siteMeta.audiences.includes(claims.a);
}

async function serveAttachment(
  sitePathSegments: string[],
  effectiveAudience: string,
  siteMeta: SiteMeta,
  env: Env,
): Promise<Response> {
  if (sitePathSegments.length < 4) {
    return notFound();
  }

  const urlAudience = sitePathSegments[1];
  const hash = sitePathSegments[2];
  const filename = sitePathSegments.slice(3).join('/');

  if (urlAudience !== effectiveAudience) {
    return forbidden('You do not have permission to access this resource.');
  }

  const key = `${siteMeta.attachment_prefix}/${hash}`;
  const object = await env.ATTACHMENTS_BUCKET.get(key);
  if (!object) {
    return notFound();
  }

  const headers = new Headers();
  headers.set('Content-Type', contentTypeForPath(filename));
  headers.set('Cache-Control', 'public, max-age=31536000, immutable');
  return new Response(object.body, { headers });
}

async function serveStaticPage(
  slug: string,
  sitePathSegments: string[],
  effectiveAudience: string,
  env: Env,
): Promise<Response> {
  // Decode percent-encoded segments (e.g. %20 → space) so R2 key matches
  // the actual stored filename (which uses literal spaces, not percent-encoding)
  let path = sitePathSegments.map((s) => decodeURIComponent(s)).join('/');
  if (!path) {
    path = 'index.html';
  }

  let key = `${slug}/${effectiveAudience}/${path}`;
  let object = await env.SITES_BUCKET.get(key);

  if (!object && !path.endsWith('/index.html')) {
    key = `${slug}/${effectiveAudience}/${path.replace(/\/$/, '')}/index.html`;
    object = await env.SITES_BUCKET.get(key);
  }

  if (!object) {
    return notFound();
  }

  const contentType = object.httpMetadata?.contentType ?? contentTypeForPath(path);
  const headers = new Headers();
  headers.set('Content-Type', contentType);
  headers.set('Cache-Control', 'public, max-age=60');

  // Generated site HTML can contain root-relative links ("/page.html"), which
  // would drop the slug prefix. Rewrite those links on the fly so routes stay
  // scoped under "/{slug}/...".
  if (contentType.toLowerCase().includes('text/html')) {
    const html = await object.text();
    const rewritten = rewriteRootRelativeUrls(html, slug);
    return new Response(rewritten, { headers });
  }

  return new Response(object.body, { headers });
}

// ---------------------------------------------------------------------------
// Namespace object serving
// ---------------------------------------------------------------------------

// Subdomains that belong to other services and should never be handled
// by the site-proxy Worker. These must match the reserved list in the
// sync server's claim_subdomain handler.
const PASSTHROUGH_SUBDOMAINS = new Set([
  'www', 'app', 'api', 'mail', 'smtp', 'ftp', 'admin', 'sync', 'publish',
]);

/** Check if a host is an infrastructure subdomain that should pass through to origin. */
function isPassthroughSubdomain(host: string): boolean {
  const suffix = `.${SITE_DOMAIN}`;
  if (!host.endsWith(suffix)) return false;
  const name = host.slice(0, -suffix.length);
  return PASSTHROUGH_SUBDOMAINS.has(name.toLowerCase());
}

/** Extract subdomain from host and look up namespace mapping in KV. */
async function resolveSubdomain(host: string, env: Env): Promise<SubdomainMapping | null> {
  // Match {name}.diaryx.org but not bare diaryx.org
  const suffix = `.${SITE_DOMAIN}`;
  if (!host.endsWith(suffix)) return null;
  const name = host.slice(0, -suffix.length);
  if (!name || name.includes('.')) return null;

  // Don't intercept infrastructure subdomains.
  if (PASSTHROUGH_SUBDOMAINS.has(name.toLowerCase())) return null;

  const raw = await env.KV.get(`subdomain:${name.toLowerCase()}`);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<SubdomainMapping>;
    if (typeof parsed.namespace_id === 'string') {
      return parsed as SubdomainMapping;
    }
  } catch {}
  return null;
}

async function resolveNamedSubdomain(
  name: string,
  env: Env,
): Promise<SubdomainMapping | null> {
  const raw = await env.KV.get(`subdomain:${name.toLowerCase()}`);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<SubdomainMapping>;
    if (typeof parsed.namespace_id === 'string') {
      return parsed as SubdomainMapping;
    }
  } catch {}
  return null;
}

/** Resolve a custom domain to a namespace mapping via KV. */
async function resolveNamespaceDomain(hostname: string, env: Env): Promise<DomainMapping | null> {
  const raw = await env.KV.get(`domain:${hostname.toLowerCase()}`);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<DomainMapping>;
    if (typeof parsed.namespace_id === 'string' && typeof parsed.audience_name === 'string') {
      return parsed as DomainMapping;
    }
  } catch {}
  return null;
}


/**
 * Check namespace audience access and return the object if allowed.
 *
 * Access control is derived from R2 object custom metadata:
 * - `audience`: the audience name this object belongs to
 * - `access`: the access level (`public`, `token`, or `private`)
 *
 * These are set by the sync server at upload time, so no sidecar
 * `_audiences.json` file is needed.
 */
async function checkNamespaceAccess(
  request: Request,
  url: URL,
  env: Env,
  nsId: string,
  r2Key: string,
  allowedAudience?: string,
): Promise<Response> {
  const object = await env.ATTACHMENTS_BUCKET.get(r2Key);
  if (!object) {
    return notFound();
  }

  // Read audience and access level from R2 custom metadata.
  const objectAudience = object.customMetadata?.audience;
  const accessLevel = object.customMetadata?.access;
  if (!objectAudience || !accessLevel) {
    // No audience/access metadata = private, not publicly accessible.
    return notFound();
  }

  // If serving via custom domain, object must match the domain's audience.
  if (allowedAudience && objectAudience !== allowedAudience) {
    return notFound();
  }

  if (accessLevel === 'public') {
    // Allowed — serve directly.
  } else if (accessLevel === 'token') {
    // Check for audience_token query param or cookie.
    const tokenStr =
      url.searchParams.get('audience_token') ??
      getCookie(request.headers.get('Cookie') ?? '', `diaryx_access_${nsId}`);

    if (!tokenStr) {
      return forbidden('Authentication required to access this resource.');
    }

    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, tokenStr);
    if (!claims || claims.s !== nsId || claims.a !== objectAudience) {
      return forbidden('This access token is invalid or has expired.');
    }
    if (claims.e !== null && claims.e < Math.floor(Date.now() / 1000)) {
      return forbidden('This access token has expired.');
    }

    // If token was in query param, redirect to strip it and set cookie.
    if (url.searchParams.has('audience_token')) {
      const redirectUrl = new URL(url.toString());
      redirectUrl.searchParams.delete('audience_token');
      const headers = new Headers();
      headers.set('Location', redirectUrl.toString());
      headers.append(
        'Set-Cookie',
        `diaryx_access_${nsId}=${encodeURIComponent(tokenStr)}; Path=/; HttpOnly; Secure; SameSite=Lax`,
      );
      return new Response(null, { status: 302, headers });
    }
  } else {
    // "private" or unknown — deny.
    return forbidden('You do not have permission to access this resource.');
  }

  // Serve the object.
  const contentType = object.httpMetadata?.contentType ?? contentTypeForPath(r2Key);
  const headers = new Headers();
  headers.set('Content-Type', contentType);
  headers.set('Cache-Control', 'public, max-age=60');
  return new Response(object.body, { headers });
}

async function getNamespaceObjectMeta(
  env: Env,
  nsId: string,
  key: string,
): Promise<NamespaceObjectMeta | null> {
  const result = await env.DB.prepare(
    `SELECT key, r2_key, mime_type, audience
       FROM namespace_objects
      WHERE namespace_id = ?1 AND key = ?2`,
  )
    .bind(nsId, key)
    .first<NamespaceObjectMeta>();

  return result ?? null;
}

async function getNamespaceAudienceMeta(
  env: Env,
  nsId: string,
  audience: string,
): Promise<NamespaceAudienceMeta | null> {
  const result = await env.DB.prepare(
    `SELECT access
       FROM namespace_audiences
      WHERE namespace_id = ?1 AND audience_name = ?2`,
  )
    .bind(nsId, audience)
    .first<NamespaceAudienceMeta>();

  return result ?? null;
}

function buildNamespaceCookie(nsId: string, token: string, path = '/'): string {
  return `diaryx_access_${nsId}=${encodeURIComponent(token)}; Path=${path}; HttpOnly; Secure; SameSite=Lax`;
}

async function serveNamespaceObjectByKey(
  request: Request,
  url: URL,
  env: Env,
  nsId: string,
  key: string,
  options?: {
    allowedAudience?: string;
    htmlPrefix?: string;
    cookiePath?: string;
  },
): Promise<Response> {
  const meta = await getNamespaceObjectMeta(env, nsId, key);
  if (!meta) {
    return notFound();
  }

  const objectAudience = meta.audience;
  if (!objectAudience) {
    return notFound();
  }
  if (options?.allowedAudience && objectAudience !== options.allowedAudience) {
    return notFound();
  }

  const audienceMeta = await getNamespaceAudienceMeta(env, nsId, objectAudience);
  if (!audienceMeta) {
    return notFound();
  }

  if (audienceMeta.access === 'public') {
    // Allowed.
  } else if (audienceMeta.access === 'token') {
    const tokenStr =
      url.searchParams.get('audience_token') ??
      getCookie(request.headers.get('Cookie') ?? '', `diaryx_access_${nsId}`);

    if (!tokenStr) {
      return forbidden('Authentication required to access this resource.');
    }

    const claims = await validateSignedToken(env.TOKEN_SIGNING_KEY, tokenStr);
    if (!claims || claims.s !== nsId || claims.a !== objectAudience) {
      return forbidden('This access token is invalid or has expired.');
    }
    if (claims.e !== null && claims.e < Math.floor(Date.now() / 1000)) {
      return forbidden('This access token has expired.');
    }

    if (url.searchParams.has('audience_token')) {
      const redirectUrl = new URL(url.toString());
      redirectUrl.searchParams.delete('audience_token');
      const headers = new Headers();
      headers.set('Location', redirectUrl.toString());
      headers.append(
        'Set-Cookie',
        buildNamespaceCookie(nsId, tokenStr, options?.cookiePath ?? '/'),
      );
      return new Response(null, { status: 302, headers });
    }
  } else {
    return forbidden('You do not have permission to access this resource.');
  }

  const blobKey = meta.r2_key ?? `ns/${nsId}/${key}`;
  const object = await env.ATTACHMENTS_BUCKET.get(blobKey);
  if (!object) {
    return notFound();
  }

  const contentType = meta.mime_type || object.httpMetadata?.contentType || contentTypeForPath(key);
  const headers = new Headers();
  headers.set('Content-Type', contentType);
  headers.set('Cache-Control', 'public, max-age=60');

  if (contentType.toLowerCase().includes('text/html')) {
    const html = await object.text();
    const rewritten = options?.htmlPrefix
      ? rewriteRootRelativeUrlsWithPrefix(html, options.htmlPrefix)
      : html;
    return new Response(rewritten, { headers });
  }

  return new Response(object.body, { headers });
}

/** Handle /ns/{ns_id}/{*path} routes. */
async function serveNamespaceRoute(
  request: Request,
  env: Env,
  url: URL,
  segments: string[],
): Promise<Response> {
  // segments = ["ns", ns_id, ...path]
  if (segments.length < 3) {
    return notFound();
  }

  const nsId = decodeURIComponent(segments[1]);
  const objectPath = segments.slice(2).map((s) => decodeURIComponent(s)).join('/');
  const r2Key = `ns/${nsId}/${objectPath}`;

  return checkNamespaceAccess(request, url, env, nsId, r2Key);
}

async function serveNamespaceSiteRoute(
  request: Request,
  env: Env,
  url: URL,
  segments: string[],
): Promise<Response> {
  if (segments.length < 3) {
    return notFound();
  }

  const nsId = decodeURIComponent(segments[1]);
  const sitePathSegments = segments.slice(2).map((s) => decodeURIComponent(s));
  const audience = sitePathSegments[0];
  if (!audience) {
    return notFound();
  }

  let objectPath = sitePathSegments.slice(1).join('/');
  if (!objectPath) {
    objectPath = 'index.html';
  }

  const key = `${audience}/${objectPath}`;
  const htmlPrefix = `/sites/${encodeURIComponent(nsId)}/${encodeURIComponent(audience)}`;
  const cookiePath = `/sites/${encodeURIComponent(nsId)}`;

  let response = await serveNamespaceObjectByKey(request, url, env, nsId, key, {
    allowedAudience: audience,
    htmlPrefix,
    cookiePath,
  });
  if (response.status !== 404) {
    return response;
  }

  if (!objectPath.endsWith('/index.html')) {
    const fallback = `${audience}/${objectPath.replace(/\/$/, '')}/index.html`;
    response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback, {
      allowedAudience: audience,
      htmlPrefix,
      cookiePath,
    });
  }

  return response.status === 404 ? notFound() : response;
}

/** Handle subdomain requests — serves the whole namespace, access control per-object.
 *
 * Audience resolution order:
 * 1. `?audience=` query parameter
 * 2. Audience prefix already in the path (e.g. `/family/page.html`)
 * 3. `default_audience` from the KV subdomain mapping
 * 4. Try to find any audience that has the requested file
 */
async function serveSubdomainSite(
  request: Request,
  env: Env,
  mapping: SubdomainMapping,
  url: URL,
): Promise<Response> {
  // Canonicalize root to "/" with trailing slash.
  const segments = url.pathname.split('/').filter(Boolean);
  if (segments.length === 0 && !url.pathname.endsWith('/')) {
    const canonicalUrl = new URL(url.toString());
    canonicalUrl.pathname = '/';
    // Preserve query params (e.g. ?audience=)
    canonicalUrl.search = url.search;
    return new Response(null, { status: 302, headers: { Location: canonicalUrl.toString() } });
  }

  const nsId = mapping.namespace_id;
  let objectPath = segments.map((s) => decodeURIComponent(s)).join('/');
  if (!objectPath) {
    objectPath = 'index.html';
  }

  // Check for ?audience= query param override.
  const queryAudience = url.searchParams.get('audience');

  // If an explicit audience is given, prepend it to the path.
  if (queryAudience) {
    const audiencedPath = `${queryAudience}/${objectPath}`;
    let response = await serveNamespaceObjectByKey(request, url, env, nsId, audiencedPath, {
      allowedAudience: queryAudience,
    });
    if (response.status === 404 && !audiencedPath.endsWith('/index.html')) {
      const fallback = `${audiencedPath.replace(/\/$/, '')}/index.html`;
      response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback, {
        allowedAudience: queryAudience,
      });
    }
    return response;
  }

  // Try the path as-is first (may already include audience prefix like /family/page.html).
  let response = await serveNamespaceObjectByKey(request, url, env, nsId, objectPath);
  if (response.status !== 404) return response;

  // Try with default_audience prefix.
  if (mapping.default_audience) {
    const audiencedPath = `${mapping.default_audience}/${objectPath}`;
    response = await serveNamespaceObjectByKey(request, url, env, nsId, audiencedPath, {
      allowedAudience: mapping.default_audience,
    });
    if (response.status !== 404) return response;
    // index.html fallback with audience prefix
    if (!audiencedPath.endsWith('/index.html')) {
      const fallback = `${audiencedPath.replace(/\/$/, '')}/index.html`;
      response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback, {
        allowedAudience: mapping.default_audience,
      });
      if (response.status !== 404) return response;
    }
  }

  // index.html fallback without audience prefix (for direct paths).
  if (!objectPath.endsWith('/index.html')) {
    const fallback = `${objectPath.replace(/\/$/, '')}/index.html`;
    response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback);
    if (response.status !== 404) return response;
  }

  return notFound();
}

/** Handle custom domain requests backed by namespace objects. */
async function serveNamespaceCustomDomain(
  request: Request,
  env: Env,
  mapping: DomainMapping,
  url: URL,
): Promise<Response> {
  const segments = url.pathname.split('/').filter(Boolean);
  let objectPath = segments.map((s) => decodeURIComponent(s)).join('/');
  if (!objectPath) {
    objectPath = 'index.html';
  }

  const nsId = mapping.namespace_id;
  const audience = mapping.audience_name;

  // Try the path as-is first (may already include audience prefix).
  let response = await serveNamespaceObjectByKey(request, url, env, nsId, objectPath, {
    allowedAudience: audience,
  });
  if (response.status !== 404) return response;

  // Try with audience prefix (e.g. /index.html → /family/index.html).
  const audiencedPath = `${audience}/${objectPath}`;
  response = await serveNamespaceObjectByKey(request, url, env, nsId, audiencedPath, {
    allowedAudience: audience,
  });
  if (response.status !== 404) return response;

  // Try index.html fallback with audience prefix.
  if (!audiencedPath.endsWith('/index.html')) {
    const fallback = `${audiencedPath.replace(/\/$/, '')}/index.html`;
    response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback, {
      allowedAudience: audience,
    });
    if (response.status !== 404) return response;
  }

  // Try index.html fallback without audience prefix.
  if (!objectPath.endsWith('/index.html')) {
    const fallback = `${objectPath.replace(/\/$/, '')}/index.html`;
    response = await serveNamespaceObjectByKey(request, url, env, nsId, fallback, {
      allowedAudience: audience,
    });
    if (response.status !== 404) return response;
  }

  return notFound();
}

// ---------------------------------------------------------------------------
// Legacy site meta
// ---------------------------------------------------------------------------

async function getSiteMeta(slug: string, env: Env): Promise<SiteMeta | null> {
  const now = Date.now();
  const cached = metaCache.get(slug);
  if (cached && cached.expiresAt > now) {
    return cached.value;
  }

  const object = await env.SITES_BUCKET.get(`${slug}/_meta.json`);
  if (!object) {
    metaCache.set(slug, { expiresAt: now + META_TTL_MS, value: null });
    return null;
  }

  const parsed = (await object.json()) as Partial<SiteMeta>;
  if (
    typeof parsed.user_id !== 'string' ||
    !Array.isArray(parsed.audiences) ||
    !Array.isArray(parsed.revoked_tokens) ||
    typeof parsed.attachment_prefix !== 'string'
  ) {
    metaCache.set(slug, { expiresAt: now + META_TTL_MS, value: null });
    return null;
  }

  const value: SiteMeta = {
    user_id: parsed.user_id,
    audiences: parsed.audiences.filter((x): x is string => typeof x === 'string'),
    revoked_tokens: parsed.revoked_tokens.filter((x): x is string => typeof x === 'string'),
    attachment_prefix: parsed.attachment_prefix,
    public_audiences: Array.isArray((parsed as any).public_audiences)
      ? (parsed as any).public_audiences.filter((x: unknown): x is string => typeof x === 'string')
      : undefined,
  };
  metaCache.set(slug, { expiresAt: now + META_TTL_MS, value });
  return value;
}

function getCookie(cookieHeader: string, name: string): string | null {
  for (const cookie of cookieHeader.split(';')) {
    const [cookieName, ...rest] = cookie.trim().split('=');
    if (cookieName === name) {
      return decodeURIComponent(rest.join('='));
    }
  }
  return null;
}

function buildCookie(slug: string, token: string, path?: string): string {
  const cookiePath = path ?? `/${slug}`;
  return `diaryx_access_${slug}=${encodeURIComponent(token)}; Path=${cookiePath}; HttpOnly; Secure; SameSite=Lax`;
}

function contentTypeForPath(path: string): string {
  const lower = path.toLowerCase();
  if (lower.endsWith('.html')) return 'text/html; charset=utf-8';
  if (lower.endsWith('.css')) return 'text/css; charset=utf-8';
  if (lower.endsWith('.js')) return 'application/javascript; charset=utf-8';
  if (lower.endsWith('.json')) return 'application/json; charset=utf-8';
  if (lower.endsWith('.svg')) return 'image/svg+xml';
  if (lower.endsWith('.png')) return 'image/png';
  if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg';
  if (lower.endsWith('.webp')) return 'image/webp';
  if (lower.endsWith('.gif')) return 'image/gif';
  if (lower.endsWith('.pdf')) return 'application/pdf';
  return 'application/octet-stream';
}

function rewriteRootRelativeUrls(html: string, slug: string): string {
  return rewriteRootRelativeUrlsWithPrefix(html, `/${slug}`);
}

function rewriteRootRelativeUrlsWithPrefix(html: string, prefix: string): string {
  const normalizedPrefix = prefix.startsWith('/') ? prefix : `/${prefix}`;
  const attrPattern = /\b(href|src|action)=(["'])\/(?!\/)([^"']*)\2/gi;
  const rewrittenAttrs = html.replace(
    attrPattern,
    (_match, attr: string, quote: string, rawPath: string) => {
      const normalized = rawPath.replace(/^\/+/, '');
      const prefixWithoutSlash = normalizedPrefix.replace(/^\/+/, '');
      if (
        normalized === prefixWithoutSlash ||
        normalized.startsWith(`${prefixWithoutSlash}/`)
      ) {
        return `${attr}=${quote}/${normalized}${quote}`;
      }
      if (normalized.length === 0) {
        return `${attr}=${quote}${normalizedPrefix}/${quote}`;
      }
      return `${attr}=${quote}${normalizedPrefix}/${normalized}${quote}`;
    },
  );

  return rewriteSrcsetUrlsWithPrefix(rewrittenAttrs, normalizedPrefix);
}

function rewriteSrcsetUrlsWithPrefix(html: string, prefix: string): string {
  const normalizedPrefix = prefix.startsWith('/') ? prefix : `/${prefix}`;
  const prefixWithoutSlash = normalizedPrefix.replace(/^\/+/, '');
  const attrPattern = /\bsrcset=(["'])([^"']*)\1/gi;

  return html.replace(attrPattern, (_match, quote: string, rawValue: string) => {
    const rewritten = rawValue
      .split(',')
      .map((candidate) => {
        const trimmed = candidate.trim();
        if (!trimmed.startsWith('/')) {
          return trimmed;
        }
        const firstSpace = trimmed.search(/\s/);
        const rawUrl = firstSpace === -1 ? trimmed : trimmed.slice(0, firstSpace);
        const descriptor = firstSpace === -1 ? '' : trimmed.slice(firstSpace);
        const normalized = rawUrl.replace(/^\/+/, '');
        if (
          normalized === prefixWithoutSlash ||
          normalized.startsWith(`${prefixWithoutSlash}/`)
        ) {
          return `/${normalized}${descriptor}`;
        }
        if (normalized.length === 0) {
          return `${normalizedPrefix}/${descriptor.trimStart()}`.trimEnd();
        }
        return `${normalizedPrefix}/${normalized}${descriptor}`;
      })
      .join(', ');

    return `srcset=${quote}${rewritten}${quote}`;
  });
}

function stripSlugPrefixFromSrcset(html: string, slug: string): string {
  const attrPattern = /\bsrcset=(["'])([^"']*)\1/gi;
  return html.replace(attrPattern, (_match, quote: string, rawValue: string) => {
    const rewritten = rawValue
      .split(',')
      .map((candidate) => {
        const trimmed = candidate.trim();
        if (!trimmed.startsWith('/')) {
          return trimmed;
        }
        const firstSpace = trimmed.search(/\s/);
        const rawUrl = firstSpace === -1 ? trimmed : trimmed.slice(0, firstSpace);
        const descriptor = firstSpace === -1 ? '' : trimmed.slice(firstSpace);
        const normalized = rawUrl.replace(/^\/+/, '');
        if (normalized === slug) {
          return `/${descriptor.trimStart()}`.trimEnd();
        }
        if (normalized.startsWith(`${slug}/`)) {
          return `/${normalized.slice(slug.length + 1)}${descriptor}`;
        }
        return trimmed;
      })
      .join(', ');

    return `srcset=${quote}${rewritten}${quote}`;
  });
}

function stripSlugPrefix(html: string, slug: string): string {
  const attrPattern = /\b(href|src|action)=(["'])\/(?!\/)([^"']*)\2/gi;

  const rewrittenAttrs = html.replace(
    attrPattern,
    (_match, attr: string, quote: string, rawPath: string) => {
      // If the path starts with the slug prefix, strip it.
      if (rawPath === slug) {
        return `${attr}=${quote}/${quote}`;
      }
      if (rawPath.startsWith(`${slug}/`)) {
        const stripped = rawPath.slice(slug.length); // keeps the leading /
        return `${attr}=${quote}${stripped}${quote}`;
      }
      // Already doesn't have slug prefix — leave as-is.
      return _match;
    },
  );

  return stripSlugPrefixFromSrcset(rewrittenAttrs, slug);
}

function notFound(): Response {
  return htmlError(404, 'Page not found', "The page you're looking for doesn't exist or may have been moved.");
}

function forbidden(message: string): Response {
  return htmlError(403, 'Access denied', message);
}

function htmlError(status: number, title: string, message: string): Response {
  const html = `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>${title}</title>
    <style>
    :root {
        --bg: #fafaf9;
        --text: #0f172a;
        --text-muted: #64748b;
        --border: #e5e7eb;
    }
    @media (prefers-color-scheme: dark) {
        :root {
            --bg: #0a0a0f;
            --text: #f1f5f9;
            --text-muted: #94a3b8;
            --border: #334155;
        }
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
        color: var(--text);
        background: var(--bg);
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 100vh;
        padding: 2rem;
        -webkit-font-smoothing: antialiased;
    }
    .error {
        text-align: center;
        max-width: 28rem;
    }
    .error-code {
        font-size: 4rem;
        font-weight: 700;
        line-height: 1;
        color: var(--text-muted);
        margin-bottom: 0.75rem;
    }
    .error-title {
        font-size: 1.25rem;
        font-weight: 600;
        margin-bottom: 0.5rem;
    }
    .error-message {
        font-size: 0.9375rem;
        color: var(--text-muted);
        line-height: 1.6;
    }
    </style>
</head>
<body>
    <div class="error">
        <div class="error-code">${status}</div>
        <h1 class="error-title">${title}</h1>
        <p class="error-message">${message}</p>
    </div>
</body>
</html>`;

  return new Response(html, {
    status,
    headers: { 'content-type': 'text/html; charset=utf-8' },
  });
}
