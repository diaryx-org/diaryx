---
title: web
description: Svelte + Tiptap frontend for Diaryx
author: adammharris
part_of: '[README](/apps/README.md)'
contents:
- '[README](/apps/web/src/README.md)'
- '[README](/apps/web/worker/README.md)'
- '[Tiptap Custom Extensions](/apps/web/docs/tiptap-custom-extensions.md)'
exclude:
- '*.lock'
- '**/*.ts'
- node_modules/**
- dist/**
- e2e/**
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

Browser-hosted cloud traffic now routes through the Cloudflare Worker in
`worker/`, which serves the SPA from Workers Static Assets and handles the
same-origin `/api/*` proxy before asset serving. The Worker keeps auth
same-origin and proxies API requests to the backend with paths passed through
unchanged (both the Cloudflare API worker and native sync server serve under `/api/`).

Production deploys attach this Worker directly to the `app.diaryx.org` custom
domain via Wrangler routes, disable `workers.dev`, and keep Preview URLs
enabled for branch/testing workflows.

### Plugin-Owned Sync

Web sync/share/provider/history/status behavior is plugin-owned:

- Runtime: `diaryx_sync_extism` loaded as `sync`
- Host rendering: generic plugin surfaces (sidebar/settings/status)
- Host responsibilities: provider routing (plugin + built-in) +
  filesystem-event-driven refresh
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

# Coverage
bun run test:coverage

# E2E tests
bun run test:e2e
```

Coverage excludes generated typings/assets, the shared shadcn-style `ui/`
primitive layer, bootstrap-only entrypoints, and files that are currently poor
fits for V8 instrumentation (for example the WASM worker bridge). When Svelte
components accumulate reusable geometry/filtering/state-transition logic, prefer
moving that logic into nearby `.ts` helpers with direct Vitest coverage rather
than forcing line coverage through large UI shells.

### Sync E2E Notes

Sync E2E tests expect a running sync server (`http://127.0.0.1:3030` by
default). The dev-mode magic-link response (`dev_link`) is used for auth flows.
If `DIARYX_SYNC_PLUGIN_WASM` is not set, the E2E builds the in-repo
`crates/plugins/diaryx_sync_extism` crate and installs that fresh guest artifact before the
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
