---
title: site-proxy worker
description: Cloudflare Worker for serving audience-gated published Diaryx sites
part_of: '[README](/README.md)'
attachments:
  - '[wrangler.jsonc](/workers/site-proxy/wrangler.jsonc)'
  - '[src/index.ts](/workers/site-proxy/src/index.ts)'
  - '[src/token.ts](/workers/site-proxy/src/token.ts)'
exclude:
  - '*.lock'
---

# Site Proxy Worker

Cloudflare Worker that serves published static sites from `diaryx-sites` and
proxies attachment blobs from `diaryx-user-data`.

## Security Flow

1. Resolve slug from `/{slug}/...`.
2. Load `{slug}/_meta.json` from the sites bucket.
3. Authenticate audience from `?access=` token or cookie.
   - On valid `?access=`, issue a `302` to the same URL without `access` and set `diaryx_access_{slug}` cookie.
   - Canonicalize `/{slug}` to `/{slug}/` so relative links in `index.html` stay scoped under the slug.
4. Enforce audience equality for attachment URLs (`/{slug}/_a/{audience}/{hash}/{filename}`).
5. Serve either static site artifact (`{slug}/{audience}/{path}`) or attachment blob (`{attachment_prefix}/{hash}`).
6. For HTML responses, rewrite root-relative `href`/`src`/`action` URLs so links remain under `/{slug}/...`.

## Local Development

```bash
cd workers/site-proxy
npm install
npx wrangler secret put TOKEN_SIGNING_KEY
npm run dev
```

`TOKEN_SIGNING_KEY` must match the sync server's `TOKEN_SIGNING_KEY`.
