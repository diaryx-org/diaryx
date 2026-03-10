---
title: Publish
description: Publishing and export UI wiring
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
---

# Publish

The Publish sidebar is now declared by the `diaryx.publish` plugin manifest and rendered through the shared declarative plugin UI pipeline.

## Files

- No dedicated Svelte wrapper lives in this directory anymore.
- Export is opened through a declarative host action button.
- Site publishing is rendered through the shared declarative host-widget path in `PluginSettingsTab.svelte`.
