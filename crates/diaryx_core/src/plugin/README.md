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

## Files

| File | Description |
|------|-------------|
| `mod.rs` | Plugin traits (`Plugin`, `WorkspacePlugin`, `FilePlugin`), `PluginId`, `PluginError`, `PluginContext` |
| `manifest.rs` | `PluginManifest`, `UiContribution`, `CliCommand`, `CliArg`, `CliArgType` |
| `events.rs` | Event types for workspace and file lifecycle hooks |
| `permissions.rs` | Permission types, config structs, and permission checking functions |
| `registry.rs` | `PluginRegistry` — collects plugins and dispatches events/commands |

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
    fn id(&self) -> PluginId { PluginId("my-plugin".into()) }
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
    plugin: "my-plugin".into(),
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
