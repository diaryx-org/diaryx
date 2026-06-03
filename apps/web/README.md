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

Browser-hosted account/publishing traffic routes through the Cloudflare Worker in
`worker/`, which serves the SPA from Workers Static Assets and handles the
same-origin `/api/*` proxy before asset serving. The Worker keeps auth
same-origin and proxies API requests to the backend with paths passed through
unchanged. The historical sync server and Cloudflare backend also contain
auth/account/publishing API code, so client changes that remove sync should not
delete those backend capabilities.

Production deploys attach this Worker directly to the `app.diaryx.org` custom
domain via Wrangler routes, disable `workers.dev`, and keep Preview URLs
enabled for branch/testing workflows.

### Workspace Storage

Diaryx workspaces are local folders. The web app asks users to create or open a
folder and then relies on external tools such as iCloud Drive, Dropbox,
Syncthing, or Git for cross-device file syncing. The client no longer exposes
workspace-provider setup, remote workspace restore, or a built-in sync button.

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

## Developer Guide

| Guide | Description |
| --- | --- |
| [TipTap Custom Extensions](docs/tiptap-custom-extensions.md) | Creating custom TipTap extensions with markdown support |
