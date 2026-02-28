# Plugin Architecture

Modular plugin system for extending Diaryx with new features.

## Overview

The plugin system provides three trait namespaces:

- **`Plugin`** — Base trait with id, init, and shutdown
- **`WorkspacePlugin`** — Workspace lifecycle events (opened, closed, changed, committed) and custom commands
- **`FilePlugin`** — Per-file lifecycle events (saved, created, deleted, moved)

Plugins are registered in the `PluginRegistry`, which is stored on the `Diaryx<FS>` struct and wired into the command handler.

## Files

| File | Description |
|------|-------------|
| `mod.rs` | Plugin traits (`Plugin`, `WorkspacePlugin`, `FilePlugin`), `PluginId`, `PluginError`, `PluginContext` |
| `events.rs` | Event types for workspace and file lifecycle hooks |
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

### Typed dispatch (handle_typed_command)

Plugins can also intercept core `Command` variants directly via `handle_typed_command` (requires `crdt` feature). This avoids JSON serialization overhead for commands that carry binary data (e.g., CRDT updates).

When `Diaryx::execute()` encounters a CRDT command, it first checks `PluginRegistry::try_typed_command()`. If a plugin returns `Some(result)`, that result is used directly. Otherwise, the command falls through to the existing inline handler code.

`SyncPlugin` in `diaryx_sync` implements this to handle all ~50 CRDT command variants, making it the authoritative CRDT handler when registered.
