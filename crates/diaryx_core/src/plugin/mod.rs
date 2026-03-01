//! Plugin architecture for Diaryx.
//!
//! Provides traits and a registry for modular feature composition.
//! Plugins can hook into workspace and file lifecycle events,
//! and handle custom commands via [`PluginCommand`](crate::command::Command::PluginCommand).
//!
//! # Plugin Namespaces
//!
//! - [`Plugin`] — base trait (id, init, shutdown)
//! - [`WorkspacePlugin`] — workspace lifecycle events + custom commands
//! - [`FilePlugin`] — per-file lifecycle events
//!
//! # Registry
//!
//! The [`PluginRegistry`] collects plugins and dispatches events/commands to them.
//! It is stored as a field on [`Diaryx`](crate::diaryx::Diaryx) and wired into
//! the command handler.

pub mod events;
pub mod manifest;
pub mod registry;

use std::fmt;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;
use ts_rs::TS;

use crate::link_parser::LinkFormat;

// Re-export key types.
pub use events::*;
pub use manifest::*;
pub use registry::PluginRegistry;

/// Unique identifier for a plugin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "bindings/")]
pub struct PluginId(pub String);

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for PluginId {
    fn from(s: &str) -> Self {
        PluginId(s.to_string())
    }
}

/// Error type for plugin operations.
#[derive(Debug, Error)]
pub enum PluginError {
    /// Plugin initialization failed.
    #[error("Plugin init failed: {0}")]
    InitFailed(String),

    /// Command handling failed.
    #[error("Plugin command error: {0}")]
    CommandError(String),

    /// Generic plugin error.
    #[error("{0}")]
    Other(String),
}

/// Context provided to plugins during initialization.
///
/// Contains runtime configuration that isn't known at plugin construction time.
/// Plugins that need filesystem access bring their own FS through generic construction
/// (type-erased at registration), so FS is intentionally NOT included here.
#[derive(Default)]
pub struct PluginContext {
    /// Workspace root directory (None if no workspace is open yet).
    pub workspace_root: Option<PathBuf>,
    /// Link format configured on the Diaryx instance.
    pub link_format: LinkFormat,
}

impl PluginContext {
    /// Create a new plugin context.
    pub fn new(workspace_root: Option<PathBuf>, link_format: LinkFormat) -> Self {
        Self {
            workspace_root,
            link_format,
        }
    }
}

// ============================================================================
// Plugin Traits
// ============================================================================

/// Base plugin trait. All plugins must implement this.
///
/// On native targets, requires `Send + Sync` for thread-safe plugin dispatch.
/// On WASM, these bounds are relaxed since everything is single-threaded.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    /// Unique identifier for this plugin.
    fn id(&self) -> PluginId;

    /// Declarative manifest describing this plugin's metadata and UI contributions.
    fn manifest(&self) -> PluginManifest;

    /// Initialize the plugin with the given context.
    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let _ = ctx;
        Ok(())
    }

    /// Shut down the plugin, releasing any resources.
    async fn shutdown(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Base plugin trait. All plugins must implement this.
///
/// WASM variant without `Send + Sync` bounds (single-threaded environment).
#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait Plugin: 'static {
    /// Unique identifier for this plugin.
    fn id(&self) -> PluginId;

    /// Declarative manifest describing this plugin's metadata and UI contributions.
    fn manifest(&self) -> PluginManifest;

    /// Initialize the plugin with the given context.
    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let _ = ctx;
        Ok(())
    }

    /// Shut down the plugin, releasing any resources.
    async fn shutdown(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Workspace lifecycle plugin.
///
/// Receives events when workspaces are opened, closed, or modified.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait WorkspacePlugin: Plugin {
    /// Called when a workspace is opened.
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        let _ = event;
    }

    /// Called when a workspace is closed.
    async fn on_workspace_closed(&self, event: &WorkspaceClosedEvent) {
        let _ = event;
    }

    /// Called when workspace metadata changes.
    async fn on_workspace_changed(&self, event: &WorkspaceChangedEvent) {
        let _ = event;
    }

    /// Called when a workspace is committed (e.g., git commit or sync checkpoint).
    async fn on_workspace_committed(&self, event: &WorkspaceCommittedEvent) {
        let _ = event;
    }

    /// Handle a plugin-specific command.
    ///
    /// Returns `None` if the command is not recognized by this plugin.
    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        let _ = (cmd, params);
        None
    }

    // ====================================================================
    // CRDT side-effect hooks (default: no-op)
    // ====================================================================

    /// Called after a workspace-modifying operation completes.
    ///
    /// Plugins that manage sync state should broadcast CRDT workspace updates
    /// to connected peers.
    async fn notify_workspace_modified(&self) {}

    /// Called when a body document needs to be renamed (file was moved/renamed).
    ///
    /// Plugins that manage body CRDTs should rename the underlying Y.Doc.
    async fn on_body_doc_renamed(&self, _old_path: &str, _new_path: &str) {}

    /// Called when a body document should be deleted.
    ///
    /// Plugins that manage body CRDTs should remove the underlying Y.Doc.
    async fn on_body_doc_deleted(&self, _path: &str) {}

    /// Called after a file operation to track CRDT metadata for echo detection.
    ///
    /// Plugins should read their own CRDT file metadata for the given canonical
    /// path and register it with the sync echo tracker.
    async fn track_file_for_sync(&self, _canonical_path: &str) {}

    /// Track body content for echo detection.
    ///
    /// Called after writing body content locally so that the sync system
    /// can recognize its own writes when they arrive via remote sync.
    fn track_content_for_sync(&self, _canonical_path: &str, _content: &str) {}

    /// Resolve a canonical path from a storage path.
    ///
    /// Returns `Some(canonical)` if the plugin performs path mapping (e.g.,
    /// guest mode prefix stripping), or `None` to use the default.
    fn get_canonical_path(&self, _storage_path: &str) -> Option<String> {
        None
    }

    /// Get the title for a file from CRDT metadata.
    ///
    /// Returns `Some(title)` if the plugin has metadata for the given path,
    /// or `None` to use the default filename-based title.
    fn get_file_title(&self, _canonical_path: &str) -> Option<String> {
        None
    }

    // ====================================================================
    // Configuration
    // ====================================================================

    /// Get this plugin's configuration (if any).
    async fn get_config(&self) -> Option<serde_json::Value> {
        None
    }

    /// Update this plugin's configuration.
    async fn set_config(&self, _config: serde_json::Value) -> std::result::Result<(), PluginError> {
        Ok(())
    }
}

/// Per-file lifecycle plugin.
///
/// Receives events when individual files are created, saved, moved, or deleted.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait FilePlugin: Plugin {
    /// Called after a file is saved.
    async fn on_file_saved(&self, event: &FileSavedEvent) {
        let _ = event;
    }

    /// Called after a file is created.
    async fn on_file_created(&self, event: &FileCreatedEvent) {
        let _ = event;
    }

    /// Called after a file is deleted.
    async fn on_file_deleted(&self, event: &FileDeletedEvent) {
        let _ = event;
    }

    /// Called after a file is moved/renamed.
    async fn on_file_moved(&self, event: &FileMovedEvent) {
        let _ = event;
    }
}
