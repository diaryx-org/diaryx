//! Pluggable sync hook system.
//!
//! Defines the [`SyncHookDelegate`] trait for auth and workspace events, and
//! [`DiarySyncHook`] — a generic siphonophore `Hook` implementation that
//! delegates to a `SyncHookDelegate` for server-specific behavior.
//!
//! Document persistence (load/save/change) is handled by [`SyncDocManager`],
//! which is shared with the Cloudflare Durable Object adapter.

use crate::doc_manager::SyncDocManager;
use async_trait::async_trait;
use siphonophore::{
    BeforeCloseDirtyPayload, BeforeSyncAction, ControlMessageResponse, Handle, Hook, HookResult,
    OnAuthenticatePayload, OnBeforeSyncPayload, OnChangePayload, OnConnectPayload,
    OnControlMessagePayload, OnDisconnectPayload, OnLoadDocumentPayload, OnPeerJoinedPayload,
    OnPeerLeftPayload, OnSavePayload,
};
use std::sync::{Arc, OnceLock};
use tracing::{debug, error, info, warn};

use crate::UpdateOrigin;
use crate::protocol::{AuthenticatedUser, DocType};
use crate::storage::StorageCache;

// ==================== SyncHookDelegate Trait ====================

/// Trait for server-specific behavior injected into the generic sync hook.
///
/// The cloud server implements this with JWT auth + session-code auth.
#[async_trait]
pub trait SyncHookDelegate: Send + Sync + 'static {
    /// Authenticate a connection request.
    ///
    /// Return `Ok(user)` to allow, `Err(reason)` to reject.
    async fn authenticate(
        &self,
        doc_id: &str,
        doc_type: &DocType,
        token: Option<&str>,
        query_params: &std::collections::HashMap<String, String>,
    ) -> Result<AuthenticatedUser, String>;

    /// Called when a peer joins a document.
    async fn on_peer_joined_extra(&self, _doc_id: &str, _user_id: &str, _peer_count: usize) {}

    /// Called when a peer leaves a document.
    async fn on_peer_left_extra(&self, _doc_id: &str, _user_id: &str, _peer_count: usize) {}
}

// ==================== DiarySyncHook ====================

/// Generic siphonophore `Hook` implementation that delegates auth and events
/// to a [`SyncHookDelegate`].
///
/// Document persistence is handled by [`SyncDocManager`] (shared with the
/// Cloudflare DO adapter). Authentication and workspace-change events are
/// delegated to the [`SyncHookDelegate`].
pub struct DiarySyncHook<D: SyncHookDelegate> {
    delegate: Arc<D>,
    storage_cache: Arc<StorageCache>,
    handle: Arc<OnceLock<Handle>>,
}

impl<D: SyncHookDelegate> DiarySyncHook<D> {
    /// Create a new DiarySyncHook.
    ///
    /// Returns the hook and a shared `OnceLock` that must be populated with the
    /// server `Handle` after `Server::with_hooks()` is called.
    pub fn new(
        delegate: Arc<D>,
        storage_cache: Arc<StorageCache>,
    ) -> (Self, Arc<OnceLock<Handle>>) {
        let handle = Arc::new(OnceLock::new());
        let hook = Self {
            delegate,
            storage_cache,
            handle: handle.clone(),
        };
        (hook, handle)
    }

    /// Get a SyncDocManager for the given workspace.
    fn doc_manager(&self, workspace_id: &str) -> Result<SyncDocManager, String> {
        let storage = self
            .storage_cache
            .get_storage(workspace_id)
            .map_err(|e| format!("Failed to get storage: {}", e))?;
        Ok(SyncDocManager::new(storage))
    }
}

#[async_trait]
impl<D: SyncHookDelegate> Hook for DiarySyncHook<D> {
    async fn on_connect(&self, payload: OnConnectPayload<'_>) -> HookResult {
        debug!(
            "Client {:?} connecting to document: {}",
            payload.client_id, payload.doc_id
        );
        Ok(())
    }

    async fn on_authenticate(&self, payload: OnAuthenticatePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let request = payload.request;

        let doc_type = DocType::parse(doc_id)
            .ok_or_else(|| format!("Invalid document ID format: {}", doc_id))?;

        let user = self
            .delegate
            .authenticate(
                doc_id,
                &doc_type,
                request.token.as_deref(),
                &request.query_params,
            )
            .await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

        info!("Authenticated user {} for doc {}", user.user_id, doc_id);
        payload.context.insert(user);
        Ok(())
    }

    async fn on_load_document(
        &self,
        payload: OnLoadDocumentPayload<'_>,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        let doc_id = payload.doc_id;
        debug!("Loading document: {}", doc_id);

        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID format: {}", doc_id);
                return Ok(None);
            }
        };

        let manager = match self.doc_manager(doc_type.workspace_id()) {
            Ok(m) => m,
            Err(e) => {
                error!("{}", e);
                return Ok(None);
            }
        };

        let storage_key = doc_type.storage_key();
        match manager.load_document(&storage_key) {
            Ok(result) => Ok(result),
            Err(e) => {
                error!("Failed to load document {}: {}", doc_id, e);
                Ok(None)
            }
        }
    }

    async fn on_change(&self, payload: OnChangePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let update = payload.update;

        debug!(
            "[on_change] doc={}, update_len={}, client={:?}",
            doc_id,
            update.len(),
            payload.client_id
        );

        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on change: {}", doc_id);
                return Ok(());
            }
        };

        let user = payload.context.get::<AuthenticatedUser>();

        // Check read-only mode
        if let Some(u) = user {
            if u.read_only {
                debug!(
                    "Ignoring change from read-only user {} on {}",
                    u.user_id, doc_id
                );
                return Ok(());
            }
        }

        let (device_id, device_name) = match user {
            Some(u) => (u.device_id.as_deref(), None),
            None => (None, None),
        };

        let manager = match self.doc_manager(doc_type.workspace_id()) {
            Ok(m) => m,
            Err(e) => {
                error!("{}", e);
                return Ok(());
            }
        };

        let storage_key = doc_type.storage_key();
        if let Err(e) = manager.apply_change(
            &storage_key,
            update,
            UpdateOrigin::Remote,
            device_id,
            device_name,
        ) {
            error!("Failed to persist update for {}: {}", doc_id, e);
        }

        Ok(())
    }

    async fn on_disconnect(&self, payload: OnDisconnectPayload<'_>) -> HookResult {
        debug!(
            "Client {:?} disconnected from document: {}",
            payload.client_id, payload.doc_id
        );
        Ok(())
    }

    async fn on_save(&self, payload: OnSavePayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let state = payload.state;

        debug!("Saving document: {} ({} bytes)", doc_id, state.len());

        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on save: {}", doc_id);
                return Ok(());
            }
        };

        let manager = self
            .doc_manager(doc_type.workspace_id())
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

        let storage_key = doc_type.storage_key();
        manager.save_document(&storage_key, state).map_err(|e| {
            error!("Failed to save document {}: {}", doc_id, e);
            format!("Save failed: {}", e)
        })?;

        Ok(())
    }

    async fn before_close_dirty(&self, payload: BeforeCloseDirtyPayload<'_>) -> HookResult {
        let doc_id = payload.doc_id;
        let state = payload.state;

        info!("Auto-saving dirty document before close: {}", doc_id);

        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                warn!("Invalid document ID on close: {}", doc_id);
                return Ok(());
            }
        };

        let manager = match self.doc_manager(doc_type.workspace_id()) {
            Ok(m) => m,
            Err(e) => {
                error!("{}", e);
                return Ok(());
            }
        };

        let storage_key = doc_type.storage_key();
        if let Err(e) = manager.save_document(&storage_key, state) {
            error!("Failed to auto-save document {}: {}", doc_id, e);
        }

        Ok(())
    }

    async fn on_before_sync(
        &self,
        payload: OnBeforeSyncPayload<'_>,
    ) -> Result<BeforeSyncAction, Box<dyn std::error::Error + Send + Sync>> {
        let doc_id = payload.doc_id;

        if DocType::parse(doc_id).is_none() {
            return Ok(BeforeSyncAction::Abort {
                reason: format!("Invalid document ID: {}", doc_id),
            });
        }

        // All doc types: no server-side handshake. Content is pushed by clients.
        Ok(BeforeSyncAction::Continue)
    }

    async fn on_control_message(
        &self,
        payload: OnControlMessagePayload<'_>,
    ) -> ControlMessageResponse {
        let message = payload.message;

        let json: serde_json::Value = match serde_json::from_str(message) {
            Ok(v) => v,
            Err(_) => return ControlMessageResponse::NotHandled,
        };

        let msg_type = json.get("type").and_then(|v| v.as_str());

        match msg_type {
            Some("file_request") => {
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    let user = payload.context.get::<AuthenticatedUser>();
                    let requester_id = user.map(|u| u.user_id.as_str()).unwrap_or("unknown");
                    let relay = crate::protocol::ServerControlMessage::FileRequested {
                        path: path.to_string(),
                        requester_id: requester_id.to_string(),
                    };
                    if let Ok(relay_json) = serde_json::to_string(&relay) {
                        if let Some(handle) = self.handle.get() {
                            if let Some(doc_id) = payload.doc_id {
                                handle
                                    .broadcast_text(doc_id, relay_json, Some(payload.client_id))
                                    .await;
                            }
                        }
                    }
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            Some("file_ready") => {
                if let Some(handle) = self.handle.get() {
                    if let Some(doc_id) = payload.doc_id {
                        handle
                            .broadcast_text(doc_id, message.to_string(), Some(payload.client_id))
                            .await;
                    }
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            Some("session_end") => {
                let ended = crate::protocol::ServerControlMessage::SessionEnded;
                if let Ok(ended_json) = serde_json::to_string(&ended) {
                    if let Some(handle) = self.handle.get() {
                        if let Some(doc_id) = payload.doc_id {
                            handle
                                .broadcast_text(doc_id, ended_json, Some(payload.client_id))
                                .await;
                        }
                    }
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            _ => ControlMessageResponse::NotHandled,
        }
    }

    async fn on_peer_joined(&self, payload: OnPeerJoinedPayload<'_>) -> HookResult {
        let user = payload.context.get::<AuthenticatedUser>();
        let user_id = user.map(|u| u.user_id.as_str()).unwrap_or("unknown");

        info!(
            "Peer {} joined document {} (total: {})",
            user_id, payload.doc_id, payload.peer_count
        );

        if let Some(handle) = self.handle.get() {
            let msg = serde_json::json!({
                "type": "peer_joined",
                "guestId": user_id,
                "peer_count": payload.peer_count,
            });
            handle
                .broadcast_text(payload.doc_id, msg.to_string(), Some(payload.client_id))
                .await;
        }

        self.delegate
            .on_peer_joined_extra(payload.doc_id, user_id, payload.peer_count)
            .await;

        Ok(())
    }

    async fn on_peer_left(&self, payload: OnPeerLeftPayload<'_>) -> HookResult {
        let user = payload.context.get::<AuthenticatedUser>();
        let user_id = user.map(|u| u.user_id.as_str()).unwrap_or("unknown");

        info!(
            "Peer {} left document {} (remaining: {})",
            user_id, payload.doc_id, payload.peer_count
        );

        if let Some(handle) = self.handle.get() {
            let msg = serde_json::json!({
                "type": "peer_left",
                "guestId": user_id,
                "peer_count": payload.peer_count,
            });
            handle
                .broadcast_text(payload.doc_id, msg.to_string(), Some(payload.client_id))
                .await;
        }

        self.delegate
            .on_peer_left_extra(payload.doc_id, user_id, payload.peer_count)
            .await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CrdtStorage;
    use siphonophore::Context;
    use yrs::{Doc, Map, ReadTxn, Transact, updates::decoder::Decode};

    struct TestDelegate;

    #[async_trait]
    impl SyncHookDelegate for TestDelegate {
        async fn authenticate(
            &self,
            _doc_id: &str,
            _doc_type: &DocType,
            _token: Option<&str>,
            _query_params: &std::collections::HashMap<String, String>,
        ) -> Result<AuthenticatedUser, String> {
            Ok(AuthenticatedUser {
                user_id: "test".into(),
                workspace_id: "test-ns".into(),
                device_id: None,
                is_guest: false,
                read_only: false,
            })
        }
    }

    #[tokio::test]
    async fn test_on_change_persists_manifest_update() {
        let tmp = tempfile::tempdir().unwrap();
        let storage_cache = Arc::new(StorageCache::new(tmp.path().to_path_buf()));
        let delegate = Arc::new(TestDelegate);
        let (hook, _handle) = DiarySyncHook::new(delegate, storage_cache.clone());

        // Create a yrs update for a manifest doc
        let doc = Doc::new();
        let map = doc.get_or_insert_map("manifest");
        {
            let mut txn = doc.transact_mut();
            map.insert(&mut txn, "notes/foo.md", "test");
        }
        let update = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());

        let mut ctx = Context::default();
        ctx.insert(AuthenticatedUser {
            user_id: "host-1".to_string(),
            workspace_id: "test-ns".to_string(),
            device_id: None,
            is_guest: false,
            read_only: false,
        });

        let doc_id = "manifest:test-ns";
        let payload = siphonophore::OnChangePayload {
            doc_id,
            client_id: kameo::actor::ActorId::new(1),
            update: &update,
            context: &ctx,
        };

        let result = Hook::on_change(&hook, payload).await;
        assert!(result.is_ok());

        // Verify the update was persisted
        let storage = storage_cache.get_storage("test-ns").unwrap();
        let updates = storage.get_all_updates("manifest:test-ns").unwrap();
        assert!(!updates.is_empty(), "manifest update should be persisted");
    }

    #[tokio::test]
    async fn test_on_change_rejects_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let storage_cache = Arc::new(StorageCache::new(tmp.path().to_path_buf()));
        let delegate = Arc::new(TestDelegate);
        let (hook, _handle) = DiarySyncHook::new(delegate, storage_cache.clone());

        let doc = Doc::new();
        let update = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());

        let mut ctx = Context::default();
        ctx.insert(AuthenticatedUser {
            user_id: "guest-1".to_string(),
            workspace_id: "test-ns".to_string(),
            device_id: None,
            is_guest: true,
            read_only: true,
        });

        let doc_id = "file:test-ns/notes/foo.md";
        let payload = siphonophore::OnChangePayload {
            doc_id,
            client_id: kameo::actor::ActorId::new(1),
            update: &update,
            context: &ctx,
        };

        let result = Hook::on_change(&hook, payload).await;
        assert!(result.is_ok());

        // Verify nothing was persisted (read-only user)
        let storage = storage_cache.get_storage("test-ns").unwrap();
        let updates = storage
            .get_all_updates("file:test-ns/notes/foo.md")
            .unwrap();
        assert!(
            updates.is_empty(),
            "read-only user changes should not be persisted"
        );
    }
}
