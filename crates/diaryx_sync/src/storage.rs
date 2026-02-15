//! Per-workspace CRDT storage cache.
//!
//! Provides a shared cache of `SqliteStorage` connections keyed by workspace ID,
//! used by both cloud and local sync servers.

use diaryx_core::crdt::SqliteStorage;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Shared cache of per-workspace `SqliteStorage` connections.
///
/// Each workspace gets its own SQLite database file at `{workspaces_dir}/{workspace_id}.db`.
/// Connections are cached and reused across hook invocations.
pub struct StorageCache {
    workspaces_dir: PathBuf,
    cache: RwLock<HashMap<String, Arc<SqliteStorage>>>,
}

impl StorageCache {
    /// Create a new StorageCache rooted at the given directory.
    pub fn new(workspaces_dir: PathBuf) -> Self {
        Self {
            workspaces_dir,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Get the workspaces directory path.
    pub fn workspaces_dir(&self) -> &PathBuf {
        &self.workspaces_dir
    }

    /// Get the path where the bare git repo for a workspace lives.
    pub fn git_repo_path(&self, workspace_id: &str) -> PathBuf {
        self.workspaces_dir.join(format!("{}.git", workspace_id))
    }

    /// Get or create storage for a workspace.
    pub fn get_storage(&self, workspace_id: &str) -> Result<Arc<SqliteStorage>, String> {
        // Check cache first
        {
            let cache = self.cache.read().unwrap();
            if let Some(storage) = cache.get(workspace_id) {
                return Ok(storage.clone());
            }
        }

        // Create new storage
        let db_path = self.workspaces_dir.join(format!("{}.db", workspace_id));
        let storage = SqliteStorage::open(&db_path)
            .map_err(|e| format!("Failed to open storage for {}: {}", workspace_id, e))?;
        let storage = Arc::new(storage);

        // Cache it
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(workspace_id.to_string(), storage.clone());
        }

        Ok(storage)
    }
}
