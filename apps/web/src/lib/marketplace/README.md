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
exclude:
  - "*.lock"
---

# Marketplace

Web marketplace domain helpers for non-WASM assets.
Curated registries follow the same pattern as `plugin-registry`: markdown source
entries assembled into a generated `registry.md` and published to CDN-backed R2.

## Files

| File | Purpose |
| --- | --- |
| `types.ts` | Shared marketplace asset types (`theme`, `typography`, `bundle`, plugin dependency metadata). |
| `themeRegistry.ts` | Trusted curated theme registry client (`registry.md` parser + validation + cache). |
| `typographyRegistry.ts` | Trusted curated typography registry client (`registry.md` parser + validation + cache). |
| `bundleRegistry.ts` | Trusted curated bundle registry client (`registry.md` parser + validation + cache). |
| `bundleApply.ts` | Bundle apply planning/execution (theme install/apply, typography preset install/apply + override apply, plugin dependency install/enable) with guided best-effort behavior. |
