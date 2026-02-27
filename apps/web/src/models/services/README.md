---
title: Services
description: Business logic services
part_of: '[README](/apps/web/src/models/README.md)'
attachments:
  - '[index.ts](/apps/web/src/models/services/index.ts)'
  - '[attachmentService.ts](/apps/web/src/models/services/attachmentService.ts)'
  - '[attachmentSyncService.ts](/apps/web/src/models/services/attachmentSyncService.ts)'
  - '[sitePublishingService.ts](/apps/web/src/models/services/sitePublishingService.ts)'
  - '[shareService.ts](/apps/web/src/models/services/shareService.ts)'
  - '[toastService.ts](/apps/web/src/models/services/toastService.ts)'
  - '[workspaceCrdtService.ts](/apps/web/src/models/services/workspaceCrdtService.ts)'
exclude:
  - '*.lock'
  - '*.test.ts'
---

# Services

Business logic services that coordinate between stores and backend.

## Files

| File | Purpose |
|------|---------|
| `attachmentService.ts` | Attachment blob URL transform/reverse + link-parser-based attachment path normalization (`canonicalizeLink`/`formatLink`) before reads. Falls back to server streaming via `getServerAttachmentUrl()` when local data is missing. Verifies local file hash against CRDT metadata and re-fetches from server on mismatch (attachment updated on another device). |
| `attachmentSyncService.ts` | Incremental/resumable attachment sync queue (multipart upload + on-demand download). Lazy sync: metadata is indexed on CRDT change but attachments are NOT auto-downloaded — users download explicitly via `enqueueAttachmentDownload()`. Exposes `getServerAttachmentUrl()` / `getServerAttachmentUrlForGuest()` for streaming attachments from the server, `getAttachmentMetadata()` for metadata lookup, `checkAttachmentIntegrity()` for hash verification, `onQueueItemStateChange()` for observing queue state, and `isAttachmentSyncEnabled()` for checking if cloud sync is active. |
| `sitePublishingService.ts` | Sync-server client for workspace site lifecycle, publish triggers, and audience token CRUD (`/api/workspaces/{id}/site*`) with deterministic status/error mapping |
| `shareService.ts` | Share session management |
| `toastService.ts` | Toast notification service |
| `workspaceCrdtService.ts` | CRDT workspace synchronization, including audience normalization (`string[]` or comma-delimited `string`) before metadata writes |
