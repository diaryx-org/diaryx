---
title: web worker
description: Cloudflare Worker entrypoint for app.diaryx.org
part_of: '[README](/apps/web/README.md)'
attachments:
  - '[index.ts](/apps/web/worker/index.ts)'
exclude:
  - '*.lock'
---

# Web Worker

Worker-native deployment entrypoint for `app.diaryx.org`.

Responsibilities:

- serve the built SPA from Workers Static Assets
- run the `/api/*` same-origin browser proxy before asset serving
- normalize the public sync route to `/api/ns/{namespace_id}/sync`
- preserve the legacy `/api/sync2?workspace_id=...` compatibility path
- forward API traffic to the current Rust sync origin during migration

Production routing is configured in `wrangler.jsonc` as a Worker Custom Domain
for `app.diaryx.org`, with Preview URLs enabled for non-production validation
and Workers observability logs enabled.
