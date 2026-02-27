//! Plugin registry for collecting and dispatching to plugins.
//!
//! The registry is the central hub that holds all registered plugins and
//! provides methods to emit events and route commands.

use std::sync::Arc;

use serde_json::Value as JsonValue;

use super::events::*;
use super::{FilePlugin, Plugin, PluginContext, PluginError, PluginId, WorkspacePlugin};

/// Central registry that holds all registered plugins.
///
/// Plugins are registered by namespace (workspace, file) and the registry
/// dispatches events and commands to the appropriate plugins.
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
    workspace_plugins: Vec<Arc<dyn WorkspacePlugin>>,
    file_plugins: Vec<Arc<dyn FilePlugin>>,
}

impl PluginRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            workspace_plugins: Vec::new(),
            file_plugins: Vec::new(),
        }
    }

    /// Register a workspace plugin.
    pub fn register_workspace_plugin(&mut self, plugin: Arc<dyn WorkspacePlugin>) {
        self.plugins.push(plugin.clone());
        self.workspace_plugins.push(plugin);
    }

    /// Register a file plugin.
    pub fn register_file_plugin(&mut self, plugin: Arc<dyn FilePlugin>) {
        self.plugins.push(plugin.clone());
        self.file_plugins.push(plugin);
    }

    /// Get all registered plugin IDs.
    pub fn plugin_ids(&self) -> Vec<PluginId> {
        self.plugins.iter().map(|p| p.id()).collect()
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Initialize all registered plugins.
    pub async fn init_all(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for plugin in &self.plugins {
            plugin.init(ctx).await?;
        }
        Ok(())
    }

    /// Shut down all registered plugins (in reverse registration order).
    pub async fn shutdown_all(&self) -> Result<(), PluginError> {
        for plugin in self.plugins.iter().rev() {
            plugin.shutdown().await?;
        }
        Ok(())
    }

    // ========================================================================
    // Workspace Events
    // ========================================================================

    /// Emit a workspace-opened event to all workspace plugins.
    pub async fn emit_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        for plugin in &self.workspace_plugins {
            plugin.on_workspace_opened(event).await;
        }
    }

    /// Emit a workspace-closed event to all workspace plugins.
    pub async fn emit_workspace_closed(&self, event: &WorkspaceClosedEvent) {
        for plugin in &self.workspace_plugins {
            plugin.on_workspace_closed(event).await;
        }
    }

    /// Emit a workspace-changed event to all workspace plugins.
    pub async fn emit_workspace_changed(&self, event: &WorkspaceChangedEvent) {
        for plugin in &self.workspace_plugins {
            plugin.on_workspace_changed(event).await;
        }
    }

    /// Emit a workspace-committed event to all workspace plugins.
    pub async fn emit_workspace_committed(&self, event: &WorkspaceCommittedEvent) {
        for plugin in &self.workspace_plugins {
            plugin.on_workspace_committed(event).await;
        }
    }

    // ========================================================================
    // File Events
    // ========================================================================

    /// Emit a file-saved event to all file plugins.
    pub async fn emit_file_saved(&self, event: &FileSavedEvent) {
        for plugin in &self.file_plugins {
            plugin.on_file_saved(event).await;
        }
    }

    /// Emit a file-created event to all file plugins.
    pub async fn emit_file_created(&self, event: &FileCreatedEvent) {
        for plugin in &self.file_plugins {
            plugin.on_file_created(event).await;
        }
    }

    /// Emit a file-deleted event to all file plugins.
    pub async fn emit_file_deleted(&self, event: &FileDeletedEvent) {
        for plugin in &self.file_plugins {
            plugin.on_file_deleted(event).await;
        }
    }

    /// Emit a file-moved event to all file plugins.
    pub async fn emit_file_moved(&self, event: &FileMovedEvent) {
        for plugin in &self.file_plugins {
            plugin.on_file_moved(event).await;
        }
    }

    // ========================================================================
    // Command Dispatch
    // ========================================================================

    /// Route a command to the matching workspace plugin.
    ///
    /// Finds the first workspace plugin whose [`PluginId`] matches `plugin_id`
    /// and calls [`WorkspacePlugin::handle_command`]. Returns `None` if no
    /// plugin matches or the matched plugin doesn't handle the command.
    pub async fn handle_plugin_command(
        &self,
        plugin_id: &str,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        for plugin in &self.workspace_plugins {
            if plugin.id().0 == plugin_id {
                return plugin.handle_command(cmd, params).await;
            }
        }
        None
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
