---
title: Sync
description: Host-side sync plugin integration services
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[pluginSyncAdapter.ts](/apps/web/src/lib/sync/pluginSyncAdapter.ts)"
  - "[workspaceProviderService.ts](/apps/web/src/lib/sync/workspaceProviderService.ts)"
  - "[attachmentSyncService.ts](/apps/web/src/lib/sync/attachmentSyncService.ts)"
exclude:
  - "*.lock"
  - "*.test.ts"
---

# Sync

Host-side adapters for the external sync plugin runtime.

The web app does not own CRDT or sync protocol logic. It hosts plugins and
routes plugin commands/events to backend APIs and UI stores.

## Files

| File | Purpose |
| --- | --- |
| `pluginSyncAdapter.ts` | WebSocket transport + event bridge for the sync plugin |
| `workspaceProviderService.ts` | Provider/workspace linking commands via sync plugin |
| `attachmentSyncService.ts` | Attachment transfer queue and metadata indexing |
