import { TokenClaims, validateSignedToken } from './token';

type Env = {
  SITES_BUCKET: R2Bucket;
  ATTACHMENTS_BUCKET: R2Bucket;
  TOKEN_SIGNING_KEY: string;
};

type SiteMeta = {
  user_id: string;
  audiences: string[];
  revoked_tokens: string[];
  attachment_prefix: string;
};

type AuthResult = {
  audience: string;
  earlyResponse?: Response;
};

const META_TTL_MS = 60_000;
const metaCache = new Map<string, { expiresAt: number; value: SiteMeta | null }>();

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const segments = url.pathname.split('/').filter(Boolean);
    if (segments.length === 0) {
      return notFound();
    }

    const slug = segments[0];
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
        earlyResponse: jsonError(403, 'invalid_access_token', 'Access token is invalid'),
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

  return { audience: 'public' };
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
    return jsonError(403, 'forbidden', 'Audience mismatch for attachment');
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
  let path = sitePathSegments.join('/');
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

function buildCookie(slug: string, token: string): string {
  return `diaryx_access_${slug}=${encodeURIComponent(token)}; Path=/${slug}; HttpOnly; Secure; SameSite=Lax`;
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
  const slugPrefix = `/${slug}`;
  const attrPattern = /\b(href|src|action)=(["'])\/(?!\/)([^"']*)\2/gi;

  return html.replace(attrPattern, (_match, attr: string, quote: string, rawPath: string) => {
    const normalized = rawPath.replace(/^\/+/, '');
    if (normalized === slug || normalized.startsWith(`${slug}/`)) {
      return `${attr}=${quote}/${normalized}${quote}`;
    }
    if (normalized.length === 0) {
      return `${attr}=${quote}${slugPrefix}/${quote}`;
    }
    return `${attr}=${quote}${slugPrefix}/${normalized}${quote}`;
  });
}

function notFound(): Response {
  return jsonError(404, 'not_found', 'Not found');
}

function jsonError(status: number, error: string, message: string): Response {
  return new Response(JSON.stringify({ error, message }), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
    },
  });
}
