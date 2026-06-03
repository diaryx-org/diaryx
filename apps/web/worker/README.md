---
title: web worker
description: Cloudflare Worker entrypoint for app.diaryx.org
part_of: '[README](/apps/web/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
---

# Web Worker

Worker-native deployment entrypoint for `app.diaryx.org`.

Responsibilities:

- serve the built SPA from Workers Static Assets
- run the `/api/*` same-origin browser proxy to forward API requests to the backend server before asset serving

Production routing is configured in `wrangler.jsonc` as a Worker Custom Domain
for `app.diaryx.org`, with Preview URLs enabled for non-production validation
and Workers observability logs enabled.
