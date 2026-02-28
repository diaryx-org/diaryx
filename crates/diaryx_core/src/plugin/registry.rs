//! Plugin registry for collecting and dispatching to plugins.
//!
//! The registry is the central hub that holds all registered plugins and
//! provides methods to emit events and route commands.

use std::sync::Arc;

use serde_json::Value as JsonValue;

use super::events::*;
use super::manifest::{PluginManifest, UiContribution};
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
    ///
    /// The plugin is added to the base `plugins` list only if no plugin with
    /// the same ID is already registered, preventing double `init()`/`shutdown()`
    /// calls and duplicate manifests when a plugin implements both
    /// `WorkspacePlugin` and `FilePlugin`.
    pub fn register_workspace_plugin(&mut self, plugin: Arc<dyn WorkspacePlugin>) {
        if !self.plugins.iter().any(|p| p.id() == plugin.id()) {
            self.plugins.push(plugin.clone());
        }
        self.workspace_plugins.push(plugin);
    }

    /// Register a file plugin.
    ///
    /// The plugin is added to the base `plugins` list only if no plugin with
    /// the same ID is already registered (see [`register_workspace_plugin`](Self::register_workspace_plugin)).
    pub fn register_file_plugin(&mut self, plugin: Arc<dyn FilePlugin>) {
        if !self.plugins.iter().any(|p| p.id() == plugin.id()) {
            self.plugins.push(plugin.clone());
        }
        self.file_plugins.push(plugin);
    }

    /// Get all registered plugin IDs.
    pub fn plugin_ids(&self) -> Vec<PluginId> {
        self.plugins.iter().map(|p| p.id()).collect()
    }

    /// Get a reference to all registered workspace plugins.
    pub fn workspace_plugins(&self) -> &[Arc<dyn WorkspacePlugin>] {
        &self.workspace_plugins
    }

    // ========================================================================
    // Manifests
    // ========================================================================

    /// Get manifests from all registered plugins.
    pub fn get_all_manifests(&self) -> Vec<PluginManifest> {
        self.plugins.iter().map(|p| p.manifest()).collect()
    }

    /// Get UI contributions from all registered plugins, tagged with plugin ID.
    pub fn get_all_ui_contributions(&self) -> Vec<(PluginId, Vec<UiContribution>)> {
        self.plugins
            .iter()
            .map(|p| {
                let m = p.manifest();
                (m.id, m.ui)
            })
            .collect()
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
    // CRDT Side-Effect Dispatch
    // ========================================================================

    /// Notify all workspace plugins that a workspace-modifying operation completed.
    ///
    /// Plugins managing sync state should broadcast CRDT workspace updates.
    pub async fn notify_workspace_modified(&self) {
        for plugin in &self.workspace_plugins {
            plugin.notify_workspace_modified().await;
        }
    }

    /// Notify all workspace plugins that a body document was renamed.
    pub async fn emit_body_doc_renamed(&self, old_path: &str, new_path: &str) {
        for plugin in &self.workspace_plugins {
            plugin.on_body_doc_renamed(old_path, new_path).await;
        }
    }

    /// Notify all workspace plugins that a body document was deleted.
    pub async fn emit_body_doc_deleted(&self, path: &str) {
        for plugin in &self.workspace_plugins {
            plugin.on_body_doc_deleted(path).await;
        }
    }

    /// Ask workspace plugins to track CRDT metadata for echo detection.
    pub async fn track_file_for_sync(&self, canonical_path: &str) {
        for plugin in &self.workspace_plugins {
            plugin.track_file_for_sync(canonical_path).await;
        }
    }

    /// Ask workspace plugins to track body content for echo detection.
    pub fn track_content_for_sync(&self, canonical_path: &str, content: &str) {
        for plugin in &self.workspace_plugins {
            plugin.track_content_for_sync(canonical_path, content);
        }
    }

    /// Resolve a canonical path from a storage path via workspace plugins.
    ///
    /// Returns the first `Some` result from any plugin, or `None` to use the default.
    pub fn get_canonical_path(&self, storage_path: &str) -> Option<String> {
        for plugin in &self.workspace_plugins {
            if let Some(canonical) = plugin.get_canonical_path(storage_path) {
                return Some(canonical);
            }
        }
        None
    }

    /// Get the title for a file from CRDT metadata via workspace plugins.
    pub fn get_file_title(&self, canonical_path: &str) -> Option<String> {
        for plugin in &self.workspace_plugins {
            if let Some(title) = plugin.get_file_title(canonical_path) {
                return Some(title);
            }
        }
        None
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

// ========================================================================
// Typed Command Dispatch
// ========================================================================

impl PluginRegistry {
    /// Try to handle a command via registered workspace plugins' typed dispatch.
    ///
    /// Iterates workspace plugins in registration order, returning the first
    /// `Some(result)` from [`WorkspacePlugin::handle_typed_command`].
    /// Returns `None` if no plugin handles the command.
    pub async fn try_typed_command(
        &self,
        cmd: &crate::command::Command,
    ) -> Option<Result<crate::command::Response, crate::error::DiaryxError>> {
        for plugin in &self.workspace_plugins {
            if let Some(result) = plugin.handle_typed_command(cmd).await {
                return Some(result);
            }
        }
        None
    }
}

// ========================================================================
// Filesystem Event Forwarding
// ========================================================================

impl PluginRegistry {
    /// Forward a filesystem event to all registered plugins.
    ///
    /// Converts a `FileSystemEvent` into the appropriate plugin events
    /// (`FileSavedEvent`, `FileCreatedEvent`, etc.) and dispatches them.
    /// This replaces CrdtFs interception when sync runs as an Extism plugin.
    pub async fn forward_fs_event(&self, event: &crate::fs::FileSystemEvent) {
        use crate::fs::FileSystemEvent;

        match event {
            FileSystemEvent::FileCreated { path, .. } => {
                let path_str = path.to_string_lossy().to_string();
                self.emit_file_created(&FileCreatedEvent { path: path_str })
                    .await;
            }
            FileSystemEvent::FileDeleted { path, .. } => {
                let path_str = path.to_string_lossy().to_string();
                self.emit_file_deleted(&FileDeletedEvent { path: path_str })
                    .await;
            }
            FileSystemEvent::FileRenamed {
                old_path, new_path, ..
            } => {
                let old = old_path.to_string_lossy().to_string();
                let new = new_path.to_string_lossy().to_string();
                self.emit_file_moved(&FileMovedEvent {
                    old_path: old,
                    new_path: new,
                })
                .await;
            }
            FileSystemEvent::FileMoved { path, .. } => {
                // FileMoved in FS events only has the new path
                let path_str = path.to_string_lossy().to_string();
                self.emit_file_saved(&FileSavedEvent { path: path_str })
                    .await;
            }
            FileSystemEvent::MetadataChanged { path, .. }
            | FileSystemEvent::ContentsChanged { path, .. } => {
                let path_str = path.to_string_lossy().to_string();
                self.emit_file_saved(&FileSavedEvent { path: path_str })
                    .await;
            }
            // Sync events and other variants are not forwarded to plugins
            _ => {}
        }
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
