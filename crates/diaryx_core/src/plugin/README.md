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
```

## Command Routing

Plugin-specific commands use the `PluginCommand` variant:

```rust
Command::PluginCommand {
    plugin: "my-plugin".into(),
    command: "do-something".into(),
    params: serde_json::json!({}),
}
```

The command handler routes these to the matching `WorkspacePlugin::handle_command`.
