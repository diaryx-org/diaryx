---
title: Marketplace
description: Marketplace registries and bundle apply execution
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[types.ts](/apps/web/src/lib/marketplace/types.ts)"
  - "[themeRegistry.ts](/apps/web/src/lib/marketplace/themeRegistry.ts)"
  - "[typographyRegistry.ts](/apps/web/src/lib/marketplace/typographyRegistry.ts)"
  - "[bundleRegistry.ts](/apps/web/src/lib/marketplace/bundleRegistry.ts)"
  - "[bundleApply.ts](/apps/web/src/lib/marketplace/bundleApply.ts)"
  - "[templateRegistry.ts](/apps/web/src/lib/marketplace/templateRegistry.ts)"
  - "[templateInstall.ts](/apps/web/src/lib/marketplace/templateInstall.ts)"
  - "[starterWorkspaceRegistry.ts](/apps/web/src/lib/marketplace/starterWorkspaceRegistry.ts)"
  - "[starterWorkspaceApply.ts](/apps/web/src/lib/marketplace/starterWorkspaceApply.ts)"
exclude:
  - "*.lock"
---

# Marketplace

Web marketplace domain helpers for non-WASM assets.
Curated registries follow the same pattern as `plugin-registry`: markdown source
entries assembled into a generated `registry.md` and published to CDN-backed R2.

`cdnBase.ts` now resolves the curated asset base differently by host:
web stays on same-origin `/cdn`, Tauri dev resolves against the local Vite
origin, and packaged Tauri builds fall back to `https://app.diaryx.org/cdn`
so native proxy fetches never receive an invalid relative URL.

## Files

| File | Purpose |
| --- | --- |
| `types.ts` | Shared marketplace asset types (`theme`, `typography`, `bundle`, `template`, `starter-workspace`, plugin dependency metadata). |
| `themeRegistry.ts` | Trusted curated theme registry client (`registry.md` parser + validation + cache). |
| `typographyRegistry.ts` | Trusted curated typography registry client (`registry.md` parser + validation + cache). |
| `bundleRegistry.ts` | Trusted curated bundle registry client (`registry.md` parser + validation + cache). |
| `bundleApply.ts` | Bundle apply planning/execution (theme install/apply, typography preset install/apply + override apply, plugin dependency install/enable) with guided best-effort behavior. |
| `templateRegistry.ts` | Trusted curated template registry client (`registry.md` parser + validation + cache). |
| `templateInstall.ts` | Template install service (fetch artifact, save to workspace `_templates/` via templating plugin). |
| `starterWorkspaceRegistry.ts` | Trusted curated starter workspace registry client (`registry.md` parser + validation + cache). |
| `starterWorkspaceApply.ts` | Starter workspace fetch service (downloads ZIP artifact from CDN for import via `backend.importFromZip()`). |
