---
title: CRDT
description: Legacy placeholder for removed web CRDT host layer
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
  - "*.test.ts"
---

# CRDT

The web app no longer contains a CRDT bridge layer.

All web-hosted CRDT/sync orchestration files that previously lived in this
folder were removed. Sync behavior is owned by the `diaryx_sync` plugin runtime
(`diaryx_sync_extism`) and exposed through plugin commands + plugin iframe UI.

## Current model

- Host responsibilities:
  - Generic plugin loading/dispatch
  - Generic plugin UI rendering (sidebar/settings/status surfaces)
  - Backend filesystem event handling and entry/tree refresh
- Sync responsibilities (plugin-owned):
  - Sync status (`GetSyncStatus`)
  - Provider flows (`GetProviderStatus`, `ListRemoteWorkspaces`, `LinkWorkspace`, `UnlinkWorkspace`, `DownloadWorkspace`)
  - Share/session flows (`CreateShareSession`, `JoinShareSession`, `EndShareSession`, `SetShareReadOnly`)
  - Sync UI (settings/share/snapshots/history) via iframe contributions
