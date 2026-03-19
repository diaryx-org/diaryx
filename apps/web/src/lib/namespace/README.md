---
title: Namespace
description: Namespace management services and host-side UI components
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[namespaceService.ts](/apps/web/src/lib/namespace/namespaceService.ts)"
  - "[namespaceContext.svelte.ts](/apps/web/src/lib/namespace/namespaceContext.svelte.ts)"
  - "[NamespaceAudienceManager.svelte](/apps/web/src/lib/namespace/NamespaceAudienceManager.svelte)"
  - "[NamespacePublishButton.svelte](/apps/web/src/lib/namespace/NamespacePublishButton.svelte)"
  - "[NamespaceSubdomainManager.svelte](/apps/web/src/lib/namespace/NamespaceSubdomainManager.svelte)"
  - "[NamespaceCustomDomainManager.svelte](/apps/web/src/lib/namespace/NamespaceCustomDomainManager.svelte)"
  - "[NamespaceSiteUrl.svelte](/apps/web/src/lib/namespace/NamespaceSiteUrl.svelte)"
  - "[NamespaceGuardWidget.svelte](/apps/web/src/lib/namespace/NamespaceGuardWidget.svelte)"
  - "[NamespaceSiteUrlWidget.svelte](/apps/web/src/lib/namespace/NamespaceSiteUrlWidget.svelte)"
  - "[NamespaceSubdomainWidget.svelte](/apps/web/src/lib/namespace/NamespaceSubdomainWidget.svelte)"
  - "[NamespaceAudienceWidget.svelte](/apps/web/src/lib/namespace/NamespaceAudienceWidget.svelte)"
  - "[NamespacePublishWidget.svelte](/apps/web/src/lib/namespace/NamespacePublishWidget.svelte)"
  - "[index.ts](/apps/web/src/lib/namespace/index.ts)"
exclude:
  - "*.lock"
---

# Namespace

Host-side namespace management extracted from the publish plugin.

Namespace operations (create namespace, manage audiences, claim subdomains,
generate tokens, custom domains) talk directly to the sync server via
`proxyFetch` instead of round-tripping through the WASM plugin guest. This
makes namespace management available to any plugin and removes the WASM
overhead for server calls.

## Architecture

- **Before**: `PublishingPanel` -> plugin command -> WASM guest -> `host::http` -> sync server
- **After**: Plugin declarative manifest -> `namespace.*` HostWidgets -> `namespaceService` -> `proxyFetch` -> sync server

Object upload/delete operations (used during publish) go through
`host::namespace::*` host functions so plugins don't need HTTP permissions.
Those host functions now share a single same-origin fetch helper in
`plugins/extismBrowserLoader.ts`, which centralizes URL normalization,
credential handling, and error translation for namespace HTTP calls.
On Cloudflare, those requests terminate in the app Worker at `/api/*` rather
than a legacy Pages Functions layer.

## Declarative UI Integration

The publish plugin's sidebar tab uses `ComponentRef::Declarative` with
`HostWidget` fields that reference `namespace.*` widget IDs. Each widget ID
maps to a thin wrapper component that reads from a shared `NamespaceContext`
(Svelte context). The context is created by `PluginSidebarPanel.svelte`
when rendering any declarative panel.

Available widget IDs:
- `namespace.guard` — Auth/workspace guards, error display, loading state
- `namespace.site-url` — Site URL display with copy button
- `namespace.subdomain` — Subdomain claim/release
- `namespace.audiences` — Audience list with access control + manage modal
- `namespace.publish-button` — Publish button with loading states
- `namespace.custom-domains` — Custom domain CRUD

## Files

| File | Purpose |
| --- | --- |
| `namespaceService.ts` | Direct API client for namespace CRUD using `proxyFetch` |
| `namespaceContext.svelte.ts` | Shared reactive context store for namespace widgets |
| **Primitives** | |
| `NamespaceAudienceManager.svelte` | Audience list with access control dialog |
| `NamespacePublishButton.svelte` | Publish button with loading states |
| `NamespaceSubdomainManager.svelte` | Subdomain claim/release UI |
| `NamespaceCustomDomainManager.svelte` | Custom domain CRUD |
| `NamespaceSiteUrl.svelte` | Site URL display with copy button |
| **Host Widget Wrappers** | |
| `NamespaceGuardWidget.svelte` | `namespace.guard` — Auth guards + triggers loading |
| `NamespaceSiteUrlWidget.svelte` | `namespace.site-url` — Reads from context |
| `NamespaceSubdomainWidget.svelte` | `namespace.subdomain` — Reads from context |
| `NamespaceAudienceWidget.svelte` | `namespace.audiences` — Private state + audience manager |
| `NamespacePublishWidget.svelte` | `namespace.publish-button` — Reads from context |
| `index.ts` | Barrel exports |
