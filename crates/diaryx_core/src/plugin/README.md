# Plugin Architecture

Modular plugin system for extending Diaryx with new features.

## Overview

The plugin system provides three trait namespaces:

- **`Plugin`** — Base trait with id, init, and shutdown
- **`WorkspacePlugin`** — Workspace lifecycle events (opened, closed, changed, committed) and custom commands
- **`FilePlugin`** — Per-file lifecycle events (saved, created, deleted, moved)

Plugins can also declare host UI surface ownership in their manifest via
`UiContribution`:

- `CommandPalette` — plugin-owned command palette surface UI
- `ContextMenu` — plugin-owned context menu UI (currently `LeftSidebarTree` target)

Plugins are registered in the `PluginRegistry`, which is stored on the `Diaryx<FS>` struct and wired into the command handler.

Plugin IDs are canonical, namespaced dotted identifiers. First-party plugins
use the `diaryx.*` namespace (for example: `diaryx.sync`, `diaryx.publish`,
`diaryx.daily`).

## Files

| File | Description |
|------|-------------|
| `mod.rs` | Plugin traits (`Plugin`, `WorkspacePlugin`, `FilePlugin`), `PluginId`, `PluginError`, `PluginContext` |
| `manifest.rs` | `PluginManifest`, `UiContribution`, `CliCommand`, marketplace types (`MarketplaceRegistry`, `MarketplaceEntry`, `PluginArtifact`, `PluginWorkspaceMetadata`) |
| `events.rs` | Event types for workspace and file lifecycle hooks |
| `permissions.rs` | Permission types, config structs, and permission checking functions |
| `registry.rs` | `PluginRegistry` — collects plugins and dispatches events/commands |

## Plugin Marketplace

The plugin marketplace uses a Diaryx-native format. Plugin guest crates live in
standalone repos under [diaryx-org](https://github.com/diaryx-org) (e.g.,
`plugin-math`, `plugin-sync`). The central
[plugin-registry](https://github.com/diaryx-org/plugin-registry) repo assembles
all plugin metadata into a single `registry.md` uploaded to CDN.

Each plugin repo has `README.md` frontmatter containing manifest metadata. The CDN
registry is a single `registry.md` file with YAML frontmatter.

### Registry Format (`registry.md`)

```yaml
---
title: "Diaryx Plugin Registry"
schema_version: 2
generated_at: "2026-03-03T00:00:00Z"
plugins:
  - id: "diaryx.sync"
    name: "Sync"
    version: "1.2.3"
    summary: "Realtime multi-device sync"
    description: "Full description..."
    author: "Diaryx Team"
    license: "PolyForm Shield 1.0.0"
    repository: "https://github.com/diaryx-org/diaryx-sync"
    categories: ["sync", "collaboration"]
    tags: ["sync", "crdt", "realtime"]
    artifact:
      url: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc.wasm"
      sha256: "abc123..."
      size: 2048000
      published_at: "2026-03-03T00:00:00Z"
    capabilities: ["sync_transport"]
---
```

### Plugin Workspace Root Format

Plugin repos use `README.md` frontmatter with `id`, `version`, `artifact`, plus
standard Diaryx frontmatter (`title`, `description`, `contents`).

### Types

- `PluginArtifact` — WASM artifact reference (`url`, `sha256`, `size`, `published_at`)
- `MarketplaceEntry` — single plugin listing with all metadata fields
- `MarketplaceRegistry` — parsed registry with `schema_version`, `generated_at`, `plugins`
- `PluginWorkspaceMetadata` — metadata parsed from a plugin workspace root

### Parsing

- `MarketplaceRegistry::from_markdown(content)` — parse `registry.md`
- `PluginWorkspaceMetadata::from_markdown(content)` — parse plugin workspace root
- `PluginWorkspaceMetadata::to_marketplace_entry()` — convert to registry entry

## Registration Dedup

When a plugin implements both `WorkspacePlugin` and `FilePlugin`, call both `register_workspace_plugin` and `register_file_plugin` with the same `Arc`. The registry deduplicates the base `plugins` list by plugin ID, so `init()`, `shutdown()`, and `manifest()` are only called once.

## PluginContext

`PluginContext` provides runtime configuration to plugins during `init()`:

- `workspace_root: Option<PathBuf>` — Workspace root directory (None if no workspace is open)
- `link_format: LinkFormat` — Link format configured on the Diaryx instance

Plugins that need filesystem access bring their own `FS` through generic construction — FS is **not** part of `PluginContext`. The generic is erased at registration via `Arc<dyn WorkspacePlugin>`.

## Usage

```rust
use std::sync::Arc;
use diaryx_core::plugin::{Plugin, WorkspacePlugin, PluginId, PluginContext, PluginError};

struct MyPlugin;

#[async_trait::async_trait]
impl Plugin for MyPlugin {
    fn id(&self) -> PluginId { PluginId("example.my-plugin".into()) }
}

#[async_trait::async_trait]
impl WorkspacePlugin for MyPlugin {
    // Override event handlers as needed
}

// Register on a Diaryx instance:
let mut diaryx = Diaryx::new(fs);
diaryx.plugin_registry_mut().register_workspace_plugin(Arc::new(MyPlugin));

// Initialize all plugins with current state:
diaryx.init_plugins().await.unwrap();
```

## Command Routing

### JSON-based (PluginCommand)

Plugin-specific commands use the `PluginCommand` variant:

```rust
Command::PluginCommand {
    plugin: "example.my-plugin".into(),
    command: "do-something".into(),
    params: serde_json::json!({}),
}
```

The command handler routes these to the matching `WorkspacePlugin::handle_command`.

## CLI Commands

Plugins can declare CLI subcommands in their manifest via `CliCommand`:

```rust
PluginManifest {
    cli: vec![CliCommand {
        name: "publish".into(),
        about: "Publish workspace as HTML".into(),
        native_handler: Some("publish".into()),
        args: vec![CliArg { name: "destination".into(), required: true, .. }],
        ..Default::default()
    }],
    ..
}
```

The CLI discovers installed plugin manifests at startup and dynamically builds
clap commands from `CliCommand` declarations. Commands with a `native_handler`
are dispatched to registered native Rust functions; pure WASM commands are
dispatched to the plugin's `handle_command` export.

## Plugin-owned UI Surfaces

When a plugin contributes `UiContribution::CommandPalette`, the web host renders
that component as the command palette UI instead of the built-in command list.

When a plugin contributes `UiContribution::ContextMenu { target: LeftSidebarTree, ... }`,
the web host routes left-sidebar tree context menu interactions to the plugin-owned
surface component.

## WorkspaceProvider Slot

When a plugin contributes `UiContribution::WorkspaceProvider`, the web host shows
that plugin as an option in the workspace creation dialog's "Sync" dropdown and
in the workspace management "Link to provider" button. The host queries provider
readiness via `getProviderStatus()` and delegates link/unlink/download operations
to `workspaceProviderService.ts`.

## Permission System

Plugins are sandboxed via a permission model stored in the workspace root index
frontmatter under a `plugins` key. Each plugin has an entry with `download` URL
and `permissions` object.

Plugin manifests may optionally declare:

- `requested_permissions.defaults` — default rules the plugin asks to install
- `requested_permissions.reasons` — human-readable rationale per permission key

Hosts can inspect this at install time and show an approval dialog before
writing defaults into root frontmatter.

### Permission Types

| Permission | Covers | Scope values |
|------------|--------|--------------|
| `read_files` | `host_read_file`, `host_list_files`, `host_file_exists` | file/folder links, `all` |
| `edit_files` | `host_write_file` (existing), `SaveEntry` | file/folder links, `all` |
| `create_files` | `CreateEntry`, `host_write_file` (new) | folder links, `all` |
| `delete_files` | `DeleteEntry` | file/folder links, `all` |
| `move_files` | `MoveEntry`, `RenameEntry` | file/folder links, `all` |
| `http_requests` | `host_http_request` | domain patterns, `all` |
| `plugin_storage` | `host_storage_get`, `host_storage_set`, `host_run_wasi_module` | `all` |

### Resolution Rules

- `all` in include = allow everything (except explicit excludes)
- Folder links = allow all descendants
- File links = allow that specific file (and siblings in same dir)
- Exclude wins over include
- Missing permission type = not configured (triggers permission UI)
- Missing plugin entry = not configured

### Enforcement

On native (Extism): each host function checks a `PermissionChecker` in
`HostContext` before proceeding. `HostContext::with_fs()` now defaults to
deny-all. CLI and Tauri attach a `FrontmatterPermissionChecker`, which reads
`plugins` from the workspace root frontmatter on each check.

On browser: `extismBrowserLoader.ts` host functions check permissions via
the `permissionStore`, showing a `PermissionBanner` for user approval when
rules are missing.

Both hosts split write access by existence:

- existing path → `edit_files`
- new path → `create_files`

Plugin storage keys are plugin-scoped (`{plugin_id}:{key}`) to avoid
cross-plugin collisions and accidental data sharing.

### YAML Example

```yaml
plugins:
  diaryx.ai:
    download: 'https://cdn.diaryx.org/plugins/diaryx_ai'
    permissions:
      read_files:
        include:
          - '[Daily](/journal/daily/daily.md)'
        exclude:
          - '[Sensitive](/private/sensitive.md)'
      http_requests:
        include:
          - 'openrouter.ai'
      plugin_storage:
        include: [all]
```

## StorageProvider Slot

Plugins can contribute `UiContribution::StorageProvider` to appear as storage
backend options in the workspace storage settings. Each provider specifies an
`id`, `label`, optional `icon`, and optional `description`.

When the user selects a plugin storage provider, the host sets the workspace's
`storageType` to `'plugin'` and stores the plugin ID in
`pluginMetadata.storage.pluginId`. On backend init, the host creates a
`JsFileSystem`-backed `DiaryxBackend` that dispatches filesystem operations
(ReadFile, WriteFile, etc.) to the plugin via `pluginFileSystem.ts`.

Plugin storage runs on the main thread (not in a Web Worker) because
`dispatchCommand()` requires main-thread access to the Extism plugin manager.

## BlockPickerItem Slot

Plugins can contribute items to the editor's block picker "More" submenu via
`UiContribution::BlockPickerItem`. Each item specifies an `editor_command` to
call, optional static `params`, and an optional `prompt` that collects user
input before execution.

The host renders contributed items dynamically — they appear when the plugin
is enabled and disappear when disabled.

## EditorExtension Slot

Plugins can contribute TipTap editor extensions via `UiContribution::EditorExtension`.
Three `EditorNodeType` variants are supported:

- **`InlineAtom`** — Inline atom node (e.g., inline math `$...$`). Requires
  `render_export` and `edit_mode`. The host generates a TipTap `Node` with a
  Svelte node view that calls the plugin's WASM render function.
- **`BlockAtom`** — Block atom node (e.g., block math `$$...$$`). Same as
  `InlineAtom` but renders as a block element.
- **`InlineMark`** — Inline mark that wraps rich text (e.g., spoiler `||text||`).
  No `render_export` needed. The host generates a TipTap `Mark` with input/paste
  rules, optional `keyboard_shortcut`, and optional `click_behavior` (e.g.,
  `ToggleClass` for hidden/revealed states). CSS is injected from the manifest.
