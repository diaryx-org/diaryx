---
title: Services
description: Business logic services
part_of: '[README](/apps/web/src/models/README.md)'
attachments:
  - '[index.ts](/apps/web/src/models/services/index.ts)'
  - '[attachmentService.ts](/apps/web/src/models/services/attachmentService.ts)'
  - '[attachmentSyncService.ts](/apps/web/src/models/services/attachmentSyncService.ts)'
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
| `attachmentService.ts` | Attachment upload and download |
| `attachmentSyncService.ts` | Incremental/resumable attachment sync queue (multipart upload + missing-blob download), including sync-storage usage refresh after successful upload completion |
| `shareService.ts` | Share session management |
| `toastService.ts` | Toast notification service |
| `workspaceCrdtService.ts` | CRDT workspace synchronization |
