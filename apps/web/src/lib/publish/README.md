---
title: Publish
description: Publishing and export UI components
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[PublishTab.svelte](/apps/web/src/lib/publish/PublishTab.svelte)"
  - "[publishBuiltinUiRegistry.ts](/apps/web/src/lib/publish/publishBuiltinUiRegistry.ts)"
exclude:
  - "*.lock"
---

# Publish

Publish components for workspace export and static-site publishing. These live in the **left sidebar** Publish tab (workspace-level concern).

## Files

| File                          | Purpose                                                                 |
| ----------------------------- | ----------------------------------------------------------------------- |
| `PublishTab.svelte`           | Publish top-level panel shell (`Export` and `Site Publishing`)          |
| `publishBuiltinUiRegistry.ts` | Built-in component ID mapping for plugin-contributed publish tab wiring |
