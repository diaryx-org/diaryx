---
title: Sync
description: Host-side sync plugin integration services
part_of: "[README](/apps/web/src/lib/README.md)"
attachments:
  - "[extismBrowserLoader.ts](/apps/web/src/lib/plugins/extismBrowserLoader.ts)"
  - "[providerPluginCommands.ts](/apps/web/src/lib/sync/providerPluginCommands.ts)"
  - "[workspaceProviderService.ts](/apps/web/src/lib/sync/workspaceProviderService.ts)"
  - "[attachmentSyncService.ts](/apps/web/src/lib/sync/attachmentSyncService.ts)"
exclude:
  - "*.lock"
  - "*.test.ts"
---

# Sync

Host-side adapters for the external sync plugin runtime.

The web app does not own CRDT or sync protocol logic. It hosts plugins and
routes plugin commands/events to backend APIs and UI stores. Workspace link
state is tracked as provider-generic metadata in the local workspace registry,
so the host only needs opaque `{ pluginId, remoteWorkspaceId }` links.

Provider command dispatch is now a thin wrapper that adds `provider_id`.
Provider guests resolve `server_url`, `auth_token`, and current workspace link
state from the generic host runtime context. The host only passes an explicit
`workspace_root` when a provider operation targets a known local workspace that
is not necessarily the one currently open in the runtime context. Host UI
surfaces also treat provider IDs as opaque links instead of special-casing
`diaryx.sync` in status rendering or overwrite flows.

Browser snapshot upload relies on the Extism host's filesystem-tree flattening
to include index-backed directories. The browser loader therefore treats root
`README.md` / `index.md` nodes as real files when answering `host_list_files`,
so provider snapshots include workspace roots instead of uploading empty ZIPs
for freshly created workspaces.

For live sync, the host forwards only generic file events. That includes
emitting `file_saved` after non-title frontmatter edits so the guest can
refresh workspace metadata and propagate create, rename, move, metadata, and
body changes across connected clients. The browser Playwright E2E now covers
that full two-client propagation path. The guest now persists body/workspace
CRDT checkpoints during those mutations and rehydrates focused files from
plugin storage on `file_opened`, so a browser refresh does not need the host
to special-case sync state restoration. `file_deleted` is also enough for the
guest to tear down focused/body-sync state for that path; once a file is
tombstoned, late body packets are ignored instead of recreating it on disk.

## Files

| File | Purpose |
| --- | --- |
| `../plugins/extismBrowserLoader.ts` | Browser Extism host functions, including sync transport bridging |
| `providerPluginCommands.ts` | Thin provider-command wrapper that adds `provider_id` and delegates execution to the guest |
| `workspaceProviderService.ts` | Provider/workspace link, snapshot upload, download bootstrap, and explicit local workspace targeting via provider plugins |
| `attachmentSyncService.ts` | Attachment transfer queue and metadata indexing |
