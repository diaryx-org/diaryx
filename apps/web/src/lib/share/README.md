---
title: Share
description: Legacy share/publish panel module
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
---

# Share

This directory no longer contains host-owned live-share session UI.

Live share/session surfaces moved to the sync plugin (`diaryx_sync_extism`) and
are rendered through plugin iframe contributions.

## Files

| File | Purpose |
| --- | --- |
| `PublishingPanel.svelte` | Thin composition wrapper composing namespace components from `$lib/namespace/`. |
