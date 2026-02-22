---
title: Controllers
description: Controller logic for UI actions
part_of: '[README](/apps/web/src/README.md)'
attachments:
  - '[index.ts](/apps/web/src/controllers/index.ts)'
  - '[attachmentController.ts](/apps/web/src/controllers/attachmentController.ts)'
  - '[commandPaletteController.ts](/apps/web/src/controllers/commandPaletteController.ts)'
  - '[entryController.ts](/apps/web/src/controllers/entryController.ts)'
  - '[linkController.ts](/apps/web/src/controllers/linkController.ts)'
  - '[workspaceController.ts](/apps/web/src/controllers/workspaceController.ts)'
exclude:
  - '*.lock'
---

# Controllers

Controller logic for UI actions, mediating between views and models.

## Files

| File | Purpose |
|------|---------|
| `attachmentController.ts` | Attachment upload/management + incremental sync enqueue + BinaryRef hash metadata updates (creates missing `BinaryRef` entries when needed, writes canonical attachment refs to CRDT metadata, and enqueues uploads with canonical metadata paths so server-side path canonicalization stays consistent for nested entries). Shows a grouped loading toast when cloud sync is active to track upload progress. |
| `commandPaletteController.ts` | Command palette actions |
| `entryController.ts` | Entry creation, editing, deletion, and frontmatter-safe property updates (normalizes `Map` frontmatter before merges/removals). Title changes delegate rename logic to the Rust backend (`SetFrontmatterProperty` handler reads workspace config for `auto_rename_to_title` and `filename_style`). |
| `linkController.ts` | Link handling and navigation |
| `workspaceController.ts` | Workspace operations (tree refresh, lazy child loading, validation). |

## Sync-time tree refresh behavior

`workspaceController.refreshTree` retries transient "workspace/file not found"
errors during sync-safe writes and avoids replacing a valid tree with a
temporary empty `.` filesystem tree. This prevents UI collapse during
snapshot import and initial body bootstrap.

`entryController.saveEntry` and `saveEntryWithSync` also retry transient write
errors (`NotFoundError`, `NoModificationAllowedError`) with escalating backoff
(100ms -> 3.2s) so autosave/manual save remain reliable during OPFS safe-write
windows.
