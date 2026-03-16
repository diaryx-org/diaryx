---
title: Services
description: Business logic services
part_of: '[README](/apps/web/src/models/README.md)'
attachments:
  - '[index.ts](/apps/web/src/models/services/index.ts)'
  - '[attachmentService.ts](/apps/web/src/models/services/attachmentService.ts)'
  - '[historyService.ts](/apps/web/src/models/services/historyService.ts)'
  - '[sitePublishingService.ts](/apps/web/src/models/services/sitePublishingService.ts)'
  - '[toastService.ts](/apps/web/src/models/services/toastService.ts)'
  - '[updaterService.ts](/apps/web/src/models/services/updaterService.ts)'
  - '[imageConverterService.ts](/apps/web/src/models/services/imageConverterService.ts)'
exclude:
  - '*.lock'
  - '*.test.ts'
---

# Services

Business logic services that coordinate between stores and backend.

## Files

| File | Purpose |
|------|---------|
| `attachmentService.ts` | Attachment blob URL transform/reverse and canonicalization helpers. |
| `historyService.ts` | History lookup helpers used by UI history surfaces. |
| `sitePublishingService.ts` | Sync-server client for workspace site lifecycle, publish triggers, and audience token CRUD (`/api/workspaces/{id}/site*`). |
| `toastService.ts` | Toast notification wrappers with consistent error/status formatting. |
| `updaterService.ts` | Tauri desktop updater check/install helpers that surface release availability through toasts without affecting web or App Store builds. |
| `imageConverterService.ts` | Plugin-backed media transcoder registry. Manages conversion plugins (e.g. HEIC→JPEG) registered via the `MediaTranscoder` capability. |

## Migration Notes

`workspaceCrdtService.ts` was removed from the web app.
Sync/share/provider operations are plugin-command-driven via the sync plugin.
Host-owned API services remain appropriate for non-CRDT domains such as site
publishing.
