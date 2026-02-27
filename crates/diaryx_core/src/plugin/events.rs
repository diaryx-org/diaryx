//! Event types for plugin lifecycle hooks.
//!
//! These events are emitted by the core system and delivered to registered
//! [`WorkspacePlugin`](super::WorkspacePlugin) and [`FilePlugin`](super::FilePlugin)
//! implementations via the [`PluginRegistry`](super::PluginRegistry).

use std::path::PathBuf;

// ============================================================================
// Workspace Events
// ============================================================================

/// Emitted when a workspace is opened.
#[derive(Debug, Clone)]
pub struct WorkspaceOpenedEvent {
    /// Root directory of the workspace.
    pub workspace_root: PathBuf,
}

/// Emitted when a workspace is closed.
#[derive(Debug, Clone)]
pub struct WorkspaceClosedEvent {
    /// Root directory of the workspace.
    pub workspace_root: PathBuf,
}

/// Emitted when workspace metadata changes (e.g., tree structure, config).
#[derive(Debug, Clone)]
pub struct WorkspaceChangedEvent {
    /// Root directory of the workspace.
    pub workspace_root: PathBuf,
    /// Paths that changed, if known.
    pub changed_paths: Vec<String>,
}

/// Emitted when a workspace is committed (git commit, sync checkpoint, etc.).
#[derive(Debug, Clone)]
pub struct WorkspaceCommittedEvent {
    /// Root directory of the workspace.
    pub workspace_root: PathBuf,
}

// ============================================================================
// File Events
// ============================================================================

/// Emitted after a file is saved.
#[derive(Debug, Clone)]
pub struct FileSavedEvent {
    /// Workspace-relative path to the file.
    pub path: String,
}

/// Emitted after a file is created.
#[derive(Debug, Clone)]
pub struct FileCreatedEvent {
    /// Workspace-relative path to the new file.
    pub path: String,
}

/// Emitted after a file is deleted.
#[derive(Debug, Clone)]
pub struct FileDeletedEvent {
    /// Workspace-relative path to the deleted file.
    pub path: String,
}

/// Emitted after a file is moved or renamed.
#[derive(Debug, Clone)]
pub struct FileMovedEvent {
    /// Previous workspace-relative path.
    pub old_path: String,
    /// New workspace-relative path.
    pub new_path: String,
}
