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
| `attachmentService.ts` | Attachment blob URL transform/reverse and canonicalization helpers, including the path resolution used by raw HTML preview media rewrites. Preview blob creation now preserves `text/html` for both direct HTML attachments like `_attachments/sample.html` and note-backed refs like `_attachments/sample.html.md`, stripping the wrapper `.md` suffix when needed so sandboxed iframe previews render instead of being treated as downloads. HTML preview blobs also inject a small host bridge that applies Diaryx theme variables and posts document height updates back to the editor so embedded HTML attachments can auto-size without modifying the source file. Drag/drop helpers now also strip accidental workspace-directory prefixes from attachment refs and reformat them through `plain_relative`, so dragged embeds serialize back to `_attachments/...` links instead of absolute local filesystem paths. |
| `toastService.ts` | Toast notification wrappers with consistent error/status formatting. |
| `updaterService.ts` | Tauri desktop updater check/install helpers that surface release availability through toasts without affecting web or App Store builds. |
| `imageConverterService.ts` | Plugin-backed media transcoder registry. Manages conversion plugins (e.g. HEIC→JPEG) registered via the `MediaTranscoder` capability. |

## Migration Notes

`workspaceCrdtService.ts` was removed from the web app.
Sync/share/provider operations are plugin-command-driven via the sync plugin.
Host-owned API services remain appropriate for non-CRDT domains such as
toast notifications and attachment management. Site publishing is now
plugin-command-driven via the publish plugin (`diaryx.publish`).
