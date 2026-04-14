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
- Export is driven through the command palette. Plugins contribute `ExportFormat` UI entries in their manifest; the host creates per-format commands (e.g. "Export as PDF"). Audience filtering inherits from the editor's audience view selector (`templateContextStore.previewAudience`).
- Export orchestration lives in `controllers/exportService.ts`.
- Site publishing is rendered through the shared declarative host-widget path in `PluginSettingsTab.svelte`.
