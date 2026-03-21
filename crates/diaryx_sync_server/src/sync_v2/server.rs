//! Siphonophore server wrapper for Diaryx cloud service.
//!
//! This module wraps the siphonophore Server with Diaryx-specific configuration,
//! using the GenericNamespaceSyncHook delegate and DiarySyncHook from the shared crate.

use axum::Router;
use diaryx_sync::hooks::DiarySyncHook;
use diaryx_sync::storage::StorageCache;
use siphonophore::Server;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::db::{AuthRepo, NamespaceRepo};

use super::generic_hook::GenericNamespaceSyncHook;

/// Wrapper for the siphonophore sync server.
pub struct SyncV2Server {
    server: Server,
}

/// State for the sync v2 server, shared with HTTP handlers.
#[derive(Clone)]
pub struct SyncV2State {
    pub handle: siphonophore::Handle,
}

impl SyncV2Server {
    /// Create a new sync server using the generic namespace hook.
    ///
    /// Authenticates via namespace ownership (JWT) or session code (guests).
    /// No attachment reconciliation — that's client-driven via plugin-sync.
    pub fn new(repo: Arc<AuthRepo>, ns_repo: Arc<NamespaceRepo>, workspaces_dir: PathBuf) -> Self {
        let storage_cache = Arc::new(StorageCache::new(workspaces_dir));
        let session_to_namespace = Arc::new(RwLock::new(HashMap::new()));

        let delegate = Arc::new(GenericNamespaceSyncHook::new(
            repo,
            ns_repo,
            session_to_namespace,
        ));

        let (hook, handle_cell) = DiarySyncHook::new(delegate, storage_cache);
        let server = Server::with_hooks(vec![Box::new(hook)]);
        handle_cell.set(server.handle()).ok();

        Self { server }
    }

    /// Get state for use with HTTP handlers.
    pub fn state(&self) -> SyncV2State {
        SyncV2State {
            handle: self.server.handle(),
        }
    }

    /// Get a router with WebSocket endpoint at the specified path.
    pub fn into_router_at(self, path: &str) -> Router {
        self.server.into_router_at(path)
    }
}
