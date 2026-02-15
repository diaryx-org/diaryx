//! Generic sync server builder.
//!
//! Wraps siphonophore `Server` with an axum `Router` for WebSocket and REST endpoints.

use crate::hooks::{DiarySyncHook, SyncHookDelegate};
use crate::protocol::DirtyWorkspaces;
use crate::storage::StorageCache;
use siphonophore::{Handle, Server};
use std::sync::Arc;

/// A configured sync server ready to be mounted as an axum `Router`.
pub struct SyncServer {
    /// The siphonophore server handle for broadcasting messages.
    pub handle: Handle,
    /// The axum router with WebSocket upgrade at `/sync2`.
    pub router: axum::Router,
}

impl SyncServer {
    /// Create a new sync server with the given delegate and storage cache.
    ///
    /// The delegate provides authentication and workspace-change hooks.
    /// Returns the configured server with its handle and axum router.
    /// The WebSocket endpoint is at `/sync2` (matching the client convention).
    pub fn new<D: SyncHookDelegate>(
        delegate: Arc<D>,
        storage_cache: Arc<StorageCache>,
        dirty_workspaces: DirtyWorkspaces,
    ) -> Self {
        let (hook, handle_lock) = DiarySyncHook::new(delegate, storage_cache, dirty_workspaces);

        let hooks: Vec<Box<dyn siphonophore::Hook>> = vec![Box::new(hook)];
        let server = Server::with_hooks(hooks);
        let handle = server.handle();
        handle_lock.set(handle.clone()).ok();

        let router = server.into_router_at("/sync2");

        Self { handle, router }
    }
}
