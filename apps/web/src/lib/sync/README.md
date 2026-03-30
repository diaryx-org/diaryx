---
title: Sync
description: Host-side sync plugin integration services
part_of: "[README](/apps/web/src/lib/README.md)"
exclude:
  - "*.lock"
  - "**/*.ts"
  - "*.test.ts"
---

# Sync

Host-side adapters for workspace providers and the external sync plugin runtime.

The web app does not own CRDT or sync protocol logic. It hosts plugins and
routes plugin commands/events to backend APIs and UI stores. Workspace link
state is tracked as provider-generic metadata in the local workspace registry,
so the host only needs opaque `{ pluginId, remoteWorkspaceId }` links.

Provider discovery can now come from either Extism plugin manifests or
host-registered built-ins. Built-ins use the same provider ID and workspace
link metadata model as plugins, but their commands are routed to host adapters
instead of `executePluginCommand(...)`.

Built-in providers can also own restore flows that do not involve a plugin
artifact at all. The current Apple/Tauri iCloud path uses a native host probe +
restore command so onboarding can attach to an existing iCloud workspace
without first migrating the current local workspace into that container.
Authenticated iCloud onboarding also creates workspace namespace metadata with
`provider: "builtin.icloud"` so other clients can discover that the workspace
exists even when they cannot open the Apple-local storage backing.
The onboarding restore UI therefore keeps those namespaces visible and marks
them unavailable on unsupported clients instead of filtering them out.

Provider command dispatch adds `provider_id` for both plugin-backed and
built-in providers. Provider guests resolve `server_url`, `auth_token`, and
current workspace link state from the generic host runtime context. The host
only passes an explicit `workspace_root` when a provider operation targets a
known local workspace that is not necessarily the one currently open in the
runtime context. Host UI surfaces also treat provider IDs as opaque links
instead of special-casing `diaryx.sync` in status rendering or overwrite flows.

Browser snapshot upload relies on the Extism host's filesystem-tree flattening
to include index-backed directories. The browser loader therefore treats root
`README.md` / `index.md` nodes as real files when answering `host_list_files`,
so provider snapshots include workspace roots instead of uploading empty ZIPs
for freshly created workspaces.

Workspace download/bootstrap now uses a longer browser-side timeout window
because provider restores pull files sequentially through the Extism host. The
host still fails fast on per-request plugin/network errors, but it avoids
aborting active restores just because a larger workspace exceeds a fixed
two-minute wall clock budget.

Bootstrap loads that use `loadPluginWithCustomInit(...)` also seed the guest's
requested permission defaults into the in-memory runtime config for that
session. That lets provider-owned restore flows write the initial downloaded
files before workspace frontmatter exists, without stalling on a hidden
permission prompt behind the launch overlay.

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
Those mutation-triggered provider commands are routed through the normal
backend/plugin-command path so the guest sees the current workspace runtime
context and linked remote workspace ID.

## Files

| File | Purpose |
| --- | --- |
| `builtinProviders.ts` | Host-registered workspace providers that only exist on specific runtimes (for example Apple/Tauri iCloud) |
| `providerRouter.ts` | Routes provider commands to either built-in host adapters or Extism plugin commands |
| `../plugins/extismBrowserLoader.ts` | Browser Extism host functions, including sync transport bridging |
| `providerPluginCommands.ts` | Thin provider-command wrapper that delegates through the provider router |
| `workspaceProviderService.ts` | Provider/workspace link, snapshot upload, download bootstrap, and explicit local workspace targeting via provider plugins |
| `attachmentSyncService.ts` | Attachment transfer queue and metadata indexing |
