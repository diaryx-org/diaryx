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
runtime.

The active product direction is folder-first: Diaryx asks the user to create or
open a local folder and leaves cross-device file movement to external sync
tools. `App.svelte`, `WelcomeScreen.svelte`, `SettingsDialog.svelte`, and
`LeftSidebar.svelte` no longer expose provider setup, remote workspace restore,
automatic sync scheduling, or manual sync controls.

The modules in this directory remain temporarily for old tests, migration
helpers, and backend cleanup work. Do not add new user-facing sync entry points
here. Future sync work should build on the local-folder model rather than
making the client manage a separate remote workspace location.

## Files

| File | Purpose |
| --- | --- |
| `builtinProviders.ts` | Legacy host-registered workspace providers that only exist on specific runtimes (for example Apple/Tauri iCloud) |
| `providerRouter.ts` | Legacy provider command router for built-in host adapters or Extism plugin commands |
| `../plugins/extismBrowserLoader.ts` | Browser Extism host functions, including legacy sync transport bridging |
| `providerPluginCommands.ts` | Legacy provider-command wrapper that delegates through the provider router |
| `workspaceProviderService.ts` | Legacy provider/workspace link, snapshot upload, download bootstrap, and explicit local workspace targeting via provider plugins |
| `syncScheduler.svelte.ts` | Legacy debounced host sync scheduler; no longer started by `App.svelte` |
| `attachmentSyncService.ts` | Legacy attachment transfer queue and metadata indexing |
