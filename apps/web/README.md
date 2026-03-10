---
title: web
description: Svelte + Tiptap frontend for Diaryx
author: adammharris
audience:
  - public
  - developers
part_of: "[README](/apps/README.md)"
contents:
  - "[README](/apps/web/src/README.md)"
  - "[Marketplace Dist](/apps/web/marketplace-dist/README.md)"
  - "[Tiptap Custom Extensions](/apps/web/docs/tiptap-custom-extensions.md)"
attachments:
  - "[package.json](/apps/web/package.json)"
  - "[vite.config.ts](/apps/web/vite.config.ts)"
  - "[svelte.config.js](/apps/web/svelte.config.js)"
  - "[tsconfig.json](/apps/web/tsconfig.json)"
  - "[vitest.config.ts](/apps/web/vitest.config.ts)"
  - "[playwright.config.ts](/apps/web/playwright.config.ts)"
  - "[components.json](/apps/web/components.json)"
  - "[index.html](/apps/web/index.html)"
exclude:
  - "*.lock"
  - "node_modules/**"
  - "dist/**"
  - "e2e/**"
---

# Diaryx Web

Svelte 5 frontend for Diaryx that runs against either the WASM backend (web)
or Tauri IPC backend (desktop shell).

## Getting Started

```bash
bun install
bun run dev
bun run build
```

## Architecture

The app uses a backend abstraction (`src/lib/backend`) so feature code can run
against WASM or Tauri with the same interface.

### Plugin-Owned Sync

Web sync/share/provider/history/status behavior is plugin-owned:

- Runtime: `diaryx_sync_extism` loaded as `sync`
- Host rendering: generic plugin surfaces (sidebar/settings/status)
- Host responsibilities: plugin dispatch + filesystem-event-driven refresh
- No host CRDT bridge layer in `apps/web/src/lib/crdt`

## Validation

Workspace validation/fixes are exposed through backend commands and used in both
WASM and Tauri runtimes.

## Testing

```bash
# Static/type checks
bun run check

# Unit tests
bunx vitest run

# E2E tests
bun run test:e2e
```

### Sync E2E Notes

Sync E2E tests expect a running sync server (`http://127.0.0.1:3030` by
default). The dev-mode magic-link response (`dev_link`) is used for auth flows.
If `DIARYX_SYNC_PLUGIN_WASM` is not set, the E2E builds the sibling
`../plugin-sync` workspace and installs that fresh guest artifact before the
browser flow starts.

The Chromium sync E2E covers the browser-hosted provider flow end to end:

- install the sync guest into both clients
- sign both clients in with the same account
- link a workspace on client A
- upload a fresh provider snapshot from client A
- download/bootstrap the linked workspace on client B
- verify client B renders the uploaded content and rejoins the sync session

Environment variables:

- `SYNC_SERVER_URL`: override sync server URL
- `SYNC_SERVER_HOST`: host for auto-started sync server (`127.0.0.1` default)
- `SYNC_SERVER_PORT`: base port for auto-started sync server (`3030` default)
- `SYNC_E2E_START_SERVER`: set `0` to skip auto-starting the sync server
- `DIARYX_SYNC_PLUGIN_WASM`: use an explicit prebuilt sync guest artifact

## Developer Guide

| Guide | Description |
| --- | --- |
| [TipTap Custom Extensions](docs/tiptap-custom-extensions.md) | Creating custom TipTap extensions with markdown support |
