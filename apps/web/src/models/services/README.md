---
title: Services
description: Business logic services
part_of: '[README](/apps/web/src/models/README.md)'
exclude:
  - '*.lock'
  - '**/*.ts'
  - '*.test.ts'
---

# Services

Business logic services that coordinate between stores and backend.

## Files

| File | Purpose |
|------|---------|
| `attachmentService.ts` | Attachment blob URL transform/reverse and canonicalization helpers, including the path resolution used by raw HTML preview media rewrites. |
| `historyService.ts` | History lookup helpers used by UI history surfaces. |
| `toastService.ts` | Toast notification wrappers with consistent error/status formatting. |
| `updaterService.ts` | Tauri desktop updater check/install helpers that surface release availability through toasts without affecting web or App Store builds. |
| `imageConverterService.ts` | Plugin-backed media transcoder registry. Manages conversion plugins (e.g. HEIC→JPEG) registered via the `MediaTranscoder` capability. |

## Migration Notes

`workspaceCrdtService.ts` was removed from the web app.
Sync/share/provider operations are plugin-command-driven via the sync plugin.
Host-owned API services remain appropriate for non-CRDT domains such as
toast notifications and attachment management. Site publishing is now
plugin-command-driven via the publish plugin (`diaryx.publish`).
