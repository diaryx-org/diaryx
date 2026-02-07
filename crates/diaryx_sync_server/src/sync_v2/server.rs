//! Siphonophore server wrapper for Diaryx.
//!
//! This module wraps the siphonophore Server with Diaryx-specific configuration.

use axum::Router;
use siphonophore::{Handle, Server};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::db::AuthRepo;

use super::hooks::DiaryxHook;
use super::store::{StorageCache, WorkspaceStore};

/// State for the sync v2 server, shared with HTTP handlers.
///
/// Provides access to the siphonophore Handle for peer counts and broadcasts,
/// a WorkspaceStore for snapshot operations, and session management.
#[derive(Clone)]
pub struct SyncV2State {
    /// Handle to the siphonophore server for peer counts and broadcasts.
    pub handle: Handle,
    /// Workspace store for snapshot export/import and file queries.
    pub store: Arc<WorkspaceStore>,
    /// Session code -> workspace ID mapping for peer count lookups and broadcasts.
    session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
}

impl SyncV2State {
    /// Get peer count for a session by looking up the workspace and querying siphonophore.
    pub async fn get_session_peer_count(&self, session_code: &str) -> Option<usize> {
        let code = session_code.to_uppercase();
        let workspace_id = self.session_to_workspace.read().await.get(&code)?.clone();
        let doc_id = format!("workspace:{}", workspace_id);
        let count = self.handle.get_peer_count(&doc_id).await;
        if count == 0 { None } else { Some(count) }
    }

    /// End a session: broadcast session_ended to all connected clients and remove mapping.
    pub async fn end_session(&self, session_code: &str) {
        let code = session_code.to_uppercase();
        let workspace_id = {
            let mut map = self.session_to_workspace.write().await;
            map.remove(&code)
        };

        if let Some(workspace_id) = workspace_id {
            let doc_id = format!("workspace:{}", workspace_id);
            let msg = serde_json::json!({"type": "session_ended"}).to_string();
            self.handle.broadcast_text(&doc_id, msg, None).await;
            info!("Ended session: {}", code);
        }
    }

    /// Register a session-to-workspace mapping.
    pub async fn register_session(&self, session_code: &str, workspace_id: &str) {
        self.session_to_workspace
            .write()
            .await
            .insert(session_code.to_uppercase(), workspace_id.to_string());
    }

    /// Get the workspace ID for a session code.
    pub async fn get_workspace_for_session(&self, session_code: &str) -> Option<String> {
        self.session_to_workspace
            .read()
            .await
            .get(&session_code.to_uppercase())
            .cloned()
    }

    /// Broadcast a read-only change to all clients connected to a session's workspace.
    pub async fn broadcast_read_only_changed(&self, session_code: &str, read_only: bool) {
        let code = session_code.to_uppercase();
        let workspace_id = {
            let map = self.session_to_workspace.read().await;
            map.get(&code).cloned()
        };

        if let Some(workspace_id) = workspace_id {
            let doc_id = format!("workspace:{}", workspace_id);
            let msg = serde_json::json!({
                "type": "read_only_changed",
                "read_only": read_only,
            })
            .to_string();
            self.handle.broadcast_text(&doc_id, msg, None).await;
        }
    }
}

/// Wrapper for the siphonophore sync server.
pub struct SyncV2Server {
    server: Server,
    storage_cache: Arc<StorageCache>,
    session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
}

impl SyncV2Server {
    /// Create a new sync v2 server with Diaryx hooks.
    pub fn new(repo: Arc<AuthRepo>, workspaces_dir: PathBuf) -> Self {
        let storage_cache = Arc::new(StorageCache::new(workspaces_dir));
        let session_to_workspace = Arc::new(RwLock::new(HashMap::new()));

        let (hook, handle_cell) =
            DiaryxHook::new(repo, storage_cache.clone(), session_to_workspace.clone());
        let server = Server::with_hooks(vec![Box::new(hook)]);
        // Set the handle so the hook can broadcast messages to clients
        handle_cell.set(server.handle()).ok();

        Self {
            server,
            storage_cache,
            session_to_workspace,
        }
    }

    /// Get state for use with HTTP handlers (api.rs, sessions.rs).
    pub fn state(&self) -> SyncV2State {
        SyncV2State {
            handle: self.server.handle(),
            store: Arc::new(WorkspaceStore::new(self.storage_cache.clone())),
            session_to_workspace: self.session_to_workspace.clone(),
        }
    }

    /// Get a router with WebSocket endpoint at the specified path.
    pub fn into_router_at(self, path: &str) -> Router {
        self.server.into_router_at(path)
    }
}
