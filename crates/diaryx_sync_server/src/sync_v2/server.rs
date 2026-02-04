//! Siphonophore server wrapper for Diaryx.
//!
//! This module wraps the siphonophore Server with Diaryx-specific configuration.

use axum::Router;
use siphonophore::{Handle, Server};
use std::path::PathBuf;
use std::sync::Arc;

use crate::db::AuthRepo;

use super::hooks::DiaryxHook;

/// State for the sync v2 server.
#[derive(Clone)]
pub struct SyncV2State {
    /// Handle to the siphonophore server for API interactions.
    pub handle: Handle,
}

/// Wrapper for the siphonophore sync server.
pub struct SyncV2Server {
    server: Server,
}

impl SyncV2Server {
    /// Create a new sync v2 server with Diaryx hooks.
    pub fn new(repo: Arc<AuthRepo>, workspaces_dir: PathBuf) -> Self {
        let hook = DiaryxHook::new(repo, workspaces_dir);
        let server = Server::with_hooks(vec![Box::new(hook)]);
        Self { server }
    }

    /// Get a handle for use in other HTTP handlers.
    pub fn handle(&self) -> Handle {
        self.server.handle()
    }

    /// Get a router with WebSocket endpoint at the specified path.
    pub fn into_router_at(self, path: &str) -> Router {
        self.server.into_router_at(path)
    }

    /// Get state for use with other handlers.
    pub fn state(&self) -> SyncV2State {
        SyncV2State {
            handle: self.server.handle(),
        }
    }
}
