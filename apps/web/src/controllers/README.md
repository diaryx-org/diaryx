---
title: Controllers
description: Controller logic for UI actions
part_of: '[README](/apps/web/src/README.md)'
exclude:
  - '*.lock'
---

# Controllers

Controller logic for UI actions, mediating between views and models.

## Files

| File | Purpose |
|------|---------|
| `attachmentController.ts` | Attachment upload/management + incremental sync enqueue + BinaryRef hash metadata updates (writes attachment bytes through the backend binary path, registers lightweight attachment refs separately, creates missing `BinaryRef` entries when needed, and enqueues uploads with canonical metadata paths so server-side path canonicalization stays consistent for nested entries). Sync hashing work is skipped entirely when the current workspace is not linked to sync, and upload flows reuse already-read bytes instead of re-reading files. Shows a grouped loading toast when cloud sync is active to track upload progress. |
| `commandPaletteController.ts` | Command palette actions |
| `entryController.ts` | Entry creation, editing, deletion, and frontmatter-safe property updates (normalizes `Map` frontmatter before merges/removals). Title changes delegate rename logic to the Rust backend (`SetFrontmatterProperty` handler reads workspace config for `auto_rename_to_title` and `filename_style`). When the renamed entry is the workspace root index, an optional `onRootIndexRenamed` callback fires so callers can sync the workspace display name to the new title. Entry-open flow supports request-scoped guards so stale `openEntry` results do not overwrite newer navigation intents. |
| `linkController.ts` | Link handling and navigation |
| `onboardingController.ts` | Onboarding orchestration (E2E bypass, starter workspace seeding, iOS first-run bootstrap, default workspace auto-creation, bundle application, welcome screen callback orchestration). Remote workspace restore now skips bundle selection and instead installs registry plugins inferred from the restored workspace root frontmatter; Apple/Tauri onboarding can also attach to the built-in iCloud provider without fetching a provider plugin artifact, and authenticated iCloud workspaces register namespace metadata for cross-device discovery. Pure .ts with dependency injection for testability. |
| `workspaceController.ts` | Workspace operations (tree refresh, lazy child loading, validation). Tree refresh now normalizes backend workspace paths that already point at a root markdown file (for example `Diaryx.md`, `README.md`, or `index.md`) before asking the backend to rediscover the root index, which avoids spurious `WorkspaceNotFound` errors on Tauri workspaces that use nonstandard root filenames. |

## Sync-time tree refresh behavior

`workspaceController.refreshTree` retries transient "workspace/file not found"
errors during sync-safe writes and avoids replacing a valid tree with a
temporary empty `.` filesystem tree. This prevents UI collapse during
snapshot import and initial body bootstrap.

`entryController.saveEntry` and `saveEntryWithSync` also retry transient write
errors (`NotFoundError`, `NoModificationAllowedError`) with escalating backoff
(100ms -> 3.2s) so autosave/manual save remain reliable during OPFS safe-write
windows.
