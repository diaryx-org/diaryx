//! Sync command handlers for the CLI.
//!
//! This module provides the `diaryx sync` command family for syncing
//! workspace metadata and file content with a remote sync server.

mod auth;
mod client;
mod progress;
mod status;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::config::Config;
use diaryx_core::crdt::{BodyDocManager, CrdtStorage, SqliteStorage, WorkspaceCrdt};

use crate::cli::args::SyncCommands;

/// Shared CRDT context for local operations.
///
/// This provides access to the CRDT storage and managers without requiring
/// network connectivity. Used by both sync commands and local operations
/// like `diaryx open` to persist changes to the CRDT.
pub struct CrdtContext {
    /// Underlying storage backend
    #[allow(dead_code)]
    pub storage: Arc<dyn CrdtStorage>,
    /// Workspace-level CRDT (file metadata index)
    pub workspace_crdt: Arc<WorkspaceCrdt>,
    /// Per-file body document manager
    pub body_manager: Arc<BodyDocManager>,
}

impl CrdtContext {
    /// Load or create a CRDT context for the given workspace.
    ///
    /// Returns `None` if the CRDT database doesn't exist yet (user hasn't used sync).
    /// Use `load_or_create` to initialize a new database if needed.
    pub fn load(workspace_root: &Path) -> Option<Self> {
        let db_path = workspace_root.join(".diaryx/crdt.db");
        if !db_path.exists() {
            return None;
        }

        Self::load_or_create(workspace_root).ok()
    }

    /// Load or create a CRDT context for the given workspace.
    ///
    /// Creates the `.diaryx` directory and `crdt.db` if they don't exist.
    pub fn load_or_create(workspace_root: &Path) -> Result<Self, String> {
        let diaryx_dir = workspace_root.join(".diaryx");
        if !diaryx_dir.exists() {
            std::fs::create_dir_all(&diaryx_dir)
                .map_err(|e| format!("Failed to create .diaryx directory: {}", e))?;
        }

        let db_path = diaryx_dir.join("crdt.db");
        let storage: Arc<dyn CrdtStorage> = Arc::new(
            SqliteStorage::open(&db_path)
                .map_err(|e| format!("Failed to open CRDT database: {}", e))?,
        );

        let workspace_crdt = Arc::new(
            WorkspaceCrdt::load(Arc::clone(&storage))
                .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
        );

        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

        Ok(Self {
            storage,
            workspace_crdt,
            body_manager,
        })
    }
}

/// Handle sync subcommands.
pub fn handle_sync_command(command: SyncCommands, workspace_override: Option<PathBuf>) {
    // Load config
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            return;
        }
    };

    // Determine workspace root
    let workspace_root = workspace_override.unwrap_or_else(|| config.default_workspace.clone());

    match command {
        SyncCommands::Login { email, server } => {
            auth::handle_login(&config, &email, server.as_deref());
        }
        SyncCommands::Verify { token, device_name } => {
            auth::handle_verify(&config, &token, device_name.as_deref());
        }
        SyncCommands::Logout => {
            auth::handle_logout(&config);
        }
        SyncCommands::Status => {
            status::handle_status(&config, &workspace_root);
        }
        SyncCommands::Start { background } => {
            if background {
                eprintln!("Background mode is not yet implemented.");
                eprintln!("Running in foreground mode instead.");
            }
            client::handle_start(&config, &workspace_root);
        }
        SyncCommands::Push { force: _ } => {
            client::handle_push(&config, &workspace_root);
        }
        SyncCommands::Pull { force: _ } => {
            client::handle_pull(&config, &workspace_root);
        }
        SyncCommands::Config {
            server,
            workspace_id,
            show,
        } => {
            status::handle_config(&config, server, workspace_id, show);
        }
    }
}
