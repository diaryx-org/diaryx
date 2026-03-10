---
title: Marketplace Views
description: Marketplace panels and plugin/theme browsing views
part_of: '[README](/apps/web/src/views/README.md)'
attachments:
  - '[MarketplaceSidebar.svelte](/apps/web/src/views/marketplace/MarketplaceSidebar.svelte)'
  - '[MarketplaceThemes.svelte](/apps/web/src/views/marketplace/MarketplaceThemes.svelte)'
  - '[MarketplaceTypography.svelte](/apps/web/src/views/marketplace/MarketplaceTypography.svelte)'
  - '[MarketplacePlugins.svelte](/apps/web/src/views/marketplace/MarketplacePlugins.svelte)'
  - '[MarketplaceBundles.svelte](/apps/web/src/views/marketplace/MarketplaceBundles.svelte)'
  - '[PluginMarketplace.svelte](/apps/web/src/views/marketplace/PluginMarketplace.svelte)'
exclude:
  - '*.lock'
---

# Marketplace Views

Marketplace UI for theme/style presets and plugin discovery/management.

## Files

| File | Purpose |
|------|---------|
| `MarketplaceSidebar.svelte` | Shared marketplace shell with section tabs |
| `MarketplaceThemes.svelte` | Theme catalog browsing with install/apply/uninstall and local import/export |
| `MarketplaceTypography.svelte` | Typography catalog browsing with install/apply/uninstall, local import/export, and per-field overrides |
| `MarketplacePlugins.svelte` | Plugin registry browsing and install/uninstall flows, including live plugin activation without a page reload and immediate local-vs-registry source reclassification after installs |
| `MarketplaceBundles.svelte` | Bundle catalog browsing with guided apply (theme + typography preset/overrides + plugin dependencies) |
| `PluginMarketplace.svelte` | Full-screen marketplace implementation (legacy/alternate surface) |
