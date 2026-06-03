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

Legacy host-side adapters for workspace providers and the external sync plugin
runtime, plus a small local attachment metadata helper.

The active product direction is folder-first: Diaryx asks the user to create or
open a local folder and leaves cross-device file movement to external sync
tools. `App.svelte`, `WelcomeScreen.svelte`, `SettingsDialog.svelte`, and
`LeftSidebar.svelte` no longer expose provider setup, remote workspace restore,
automatic sync scheduling, or manual sync controls.

The remaining provider modules are not user-facing entry points. They are kept
for plugin/runtime compatibility and migration cleanup while the product moves
to a local-folder model. Do not add new UI flows that make the client manage a
separate remote workspace location.

## Files

| File | Purpose |
| --- | --- |
| `builtinProviders.ts` | Legacy host-registered workspace providers that only exist on specific runtimes (for example Apple/Tauri iCloud) |
| `builtinIcloudProvider.ts` | Browser/Tauri iCloud provider adapter retained for compatibility tests and migration cleanup |
| `browserProviderBootstrap.ts` | Browser bootstrap helpers for provider registration |
| `browserWorkspaceMutationMirror.ts` | Browser-side mutation mirroring used by legacy provider compatibility paths |
| `deferredFileQueue.ts` | Deferred local file mutation queue for provider compatibility paths |
| `providerRouter.ts` | Legacy provider command router for built-in host adapters or Extism plugin commands |
| `providerPluginCommands.ts` | Legacy provider-command wrapper that delegates through the provider router |
| `providerTypes.ts` | Shared provider command/type definitions |
| `syncedWorkspaceRecovery.ts` | Recovery helpers for legacy synced workspace registry state |
| `attachmentSyncService.ts` | Local attachment hash + metadata indexing helper; it no longer queues uploads or downloads |
