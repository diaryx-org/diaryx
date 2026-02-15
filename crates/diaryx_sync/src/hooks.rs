//! Pluggable sync hook system.
//!
//! Defines the [`SyncHookDelegate`] trait for auth and workspace events, and
//! [`DiarySyncHook`] — a generic siphonophore `Hook` implementation that
//! delegates to a `SyncHookDelegate` for server-specific behavior.

use async_trait::async_trait;
use diaryx_core::crdt::{CrdtStorage, UpdateOrigin};
use siphonophore::{
    BeforeCloseDirtyPayload, BeforeSyncAction, ControlMessageResponse, Handle, Hook, HookResult,
    OnAuthenticatePayload, OnBeforeSyncPayload, OnChangePayload, OnConnectPayload,
    OnControlMessagePayload, OnDisconnectPayload, OnLoadDocumentPayload, OnPeerJoinedPayload,
    OnPeerLeftPayload, OnSavePayload,
};
use std::sync::{Arc, OnceLock};
use tracing::{debug, error, info, warn};

use crate::protocol::{AuthenticatedUser, DirtyWorkspaces, DocType, select_persistable_update};
use crate::storage::StorageCache;

// ==================== SyncHookDelegate Trait ====================

/// Trait for server-specific behavior injected into the generic sync hook.
///
/// The cloud server implements this with JWT auth, attachment reconciliation, etc.
/// The local CLI server implements this with no-op auth for single-workspace mode.
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

    /// Called after a workspace document changes (for git auto-commit, attachment reconciliation, etc.).
    async fn on_workspace_changed(&self, workspace_id: &str);

    /// Called when a peer joins a document. Default implementation broadcasts peer_joined.
    async fn on_peer_joined_extra(&self, _doc_id: &str, _user_id: &str, _peer_count: usize) {}

    /// Called when a peer leaves a document. Default implementation broadcasts peer_left.
    async fn on_peer_left_extra(&self, _doc_id: &str, _user_id: &str, _peer_count: usize) {}
}

// ==================== DiarySyncHook ====================

/// Generic siphonophore `Hook` implementation that delegates auth and events
/// to a [`SyncHookDelegate`].
///
/// Handles document persistence (load/save/change) using `StorageCache`,
/// and delegates authentication and workspace-change events to the delegate.
pub struct DiarySyncHook<D: SyncHookDelegate> {
    delegate: Arc<D>,
    storage_cache: Arc<StorageCache>,
    handle: Arc<OnceLock<Handle>>,
    dirty_workspaces: DirtyWorkspaces,
}

impl<D: SyncHookDelegate> DiarySyncHook<D> {
    /// Create a new DiarySyncHook.
    ///
    /// Returns the hook and a shared `OnceLock` that must be populated with the
    /// server `Handle` after `Server::with_hooks()` is called.
    pub fn new(
        delegate: Arc<D>,
        storage_cache: Arc<StorageCache>,
        dirty_workspaces: DirtyWorkspaces,
    ) -> (Self, Arc<OnceLock<Handle>>) {
        let handle = Arc::new(OnceLock::new());
        let hook = Self {
            delegate,
            storage_cache,
            handle: handle.clone(),
            dirty_workspaces,
        };
        (hook, handle)
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

        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage: {}", e);
                return Ok(None);
            }
        };

        let storage_key = doc_type.storage_key();

        // Load base snapshot from the `documents` table.
        let base_state = match storage.load_doc(&storage_key) {
            Ok(state) => state,
            Err(e) => {
                error!("Failed to load document {}: {}", doc_id, e);
                return Ok(None);
            }
        };

        // Also load incremental updates from the `updates` table, in case the
        // caller only stored data via append_update (e.g. WorkspaceCrdt::set_file).
        let updates = match storage.get_all_updates(&storage_key) {
            Ok(u) => u,
            Err(e) => {
                debug!("No incremental updates for {}: {}", doc_id, e);
                Vec::new()
            }
        };

        if base_state.is_none() && updates.is_empty() {
            debug!(
                "[on_load_document] doc={}, storage_key={}, has_state=false",
                doc_id, storage_key
            );
            return Ok(None);
        }

        // Merge base + incremental updates into a single state vector.
        use yrs::{Doc, ReadTxn, Transact, Update, updates::decoder::Decode};
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            if let Some(state) = &base_state {
                if let Ok(update) = Update::decode_v1(state) {
                    let _ = txn.apply_update(update);
                }
            }
            for crdt_update in &updates {
                if let Ok(update) = Update::decode_v1(&crdt_update.data) {
                    let _ = txn.apply_update(update);
                }
            }
        }
        let merged = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());
        debug!(
            "[on_load_document] doc={}, storage_key={}, has_state=true, state_len={} (base={}, updates={})",
            doc_id,
            storage_key,
            merged.len(),
            base_state.as_ref().map(|s| s.len()).unwrap_or(0),
            updates.len()
        );
        Ok(Some(merged))
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
        let (device_id, device_name) = match user {
            Some(u) => (u.device_id.as_deref(), None),
            None => (None, None),
        };

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

        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for change: {}", e);
                return Ok(());
            }
        };

        let (update_data, update_mode) = select_persistable_update(update);
        let update_data_ref = update_data.as_ref();

        let storage_key = doc_type.storage_key();
        if let Err(e) = storage.append_update_with_device(
            &storage_key,
            update_data_ref,
            UpdateOrigin::Remote,
            device_id,
            device_name,
        ) {
            error!("Failed to persist update for {}: {}", doc_id, e);
        } else {
            debug!(
                "Persisted {} byte update for {} (mode={})",
                update_data_ref.len(),
                doc_id,
                update_mode
            );

            // Mark workspace as dirty
            let workspace_id = doc_type.workspace_id().to_string();
            self.dirty_workspaces
                .write()
                .await
                .insert(workspace_id.clone(), tokio::time::Instant::now());

            // Notify delegate of workspace change
            if matches!(doc_type, DocType::Workspace(_)) {
                self.delegate.on_workspace_changed(&workspace_id).await;
            }
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

        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for save: {}", e);
                return Err(e.into());
            }
        };

        let storage_key = doc_type.storage_key();
        storage.save_doc(&storage_key, state).map_err(|e| {
            error!("Failed to save document {}: {}", doc_id, e);
            format!("Save failed: {}", e)
        })?;

        info!("Saved document {} ({} bytes)", doc_id, state.len());
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

        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get storage for auto-save: {}", e);
                return Ok(());
            }
        };

        let storage_key = doc_type.storage_key();
        if let Err(e) = storage.save_doc(&storage_key, state) {
            error!("Failed to auto-save document {}: {}", doc_id, e);
        } else {
            info!(
                "Auto-saved document {} on close ({} bytes)",
                doc_id,
                state.len()
            );
        }

        Ok(())
    }

    async fn on_before_sync(
        &self,
        payload: OnBeforeSyncPayload<'_>,
    ) -> Result<BeforeSyncAction, Box<dyn std::error::Error + Send + Sync>> {
        let doc_id = payload.doc_id;

        let doc_type = match DocType::parse(doc_id) {
            Some(dt) => dt,
            None => {
                return Ok(BeforeSyncAction::Abort {
                    reason: format!("Invalid document ID: {}", doc_id),
                });
            }
        };

        // Only workspace documents need Files-Ready handshake and session_joined
        if !matches!(doc_type, DocType::Workspace(_)) {
            return Ok(BeforeSyncAction::Continue);
        }

        let mut messages = Vec::new();

        // For session guests, send session_joined confirmation
        if let Some(user) = payload.context.get::<AuthenticatedUser>() {
            if user.is_guest {
                if let Some(session_code) = payload.request.query_params.get("session") {
                    let session_joined = serde_json::json!({
                        "type": "session_joined",
                        "joinCode": session_code.to_uppercase(),
                        "workspaceId": user.workspace_id,
                        "readOnly": user.read_only,
                    });
                    messages.push(session_joined.to_string());
                    info!(
                        "Sending session_joined for guest on workspace {}",
                        user.workspace_id
                    );
                }
            }
        }

        // Get storage to generate file manifest
        let storage = match self.storage_cache.get_storage(doc_type.workspace_id()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to get storage for before_sync: {}", e);
                if messages.is_empty() {
                    return Ok(BeforeSyncAction::Continue);
                }
                let manifest = serde_json::json!({
                    "type": "file_manifest",
                    "files": [],
                    "client_is_new": false
                });
                messages.push(manifest.to_string());
                return Ok(BeforeSyncAction::SendMessages { messages });
            }
        };

        // Query active files
        let files = match storage.query_active_files() {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to query files for manifest: {}", e);
                if messages.is_empty() {
                    return Ok(BeforeSyncAction::Continue);
                }
                let manifest = serde_json::json!({
                    "type": "file_manifest",
                    "files": [],
                    "client_is_new": false
                });
                messages.push(manifest.to_string());
                return Ok(BeforeSyncAction::SendMessages { messages });
            }
        };

        // If no files and no session messages, skip handshake
        if files.is_empty() && messages.is_empty() {
            debug!("No files in workspace, skipping Files-Ready handshake");
            return Ok(BeforeSyncAction::Continue);
        }

        // Generate file manifest message
        {
            let manifest = serde_json::json!({
                "type": "file_manifest",
                "files": files.iter().map(|(path, title, part_of)| {
                    serde_json::json!({
                        "doc_id": format!("body:{}/{}", doc_type.workspace_id(), path),
                        "filename": path,
                        "title": title,
                        "part_of": part_of,
                        "deleted": false
                    })
                }).collect::<Vec<_>>(),
                "client_is_new": false
            });

            if !files.is_empty() {
                info!(
                    "Sending file manifest with {} files for {}",
                    files.len(),
                    doc_id
                );
            }

            messages.push(manifest.to_string());
        }

        Ok(BeforeSyncAction::SendMessages { messages })
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
            Some("files_ready") | Some("FilesReady") => {
                debug!("Received FilesReady from client");

                if let Some(doc_id) = payload.doc_id {
                    if let Some(DocType::Workspace(workspace_id)) = DocType::parse(doc_id) {
                        if let Ok(storage) = self.storage_cache.get_storage(&workspace_id) {
                            let files_synced = storage
                                .query_active_files()
                                .map(|files| files.len())
                                .unwrap_or(0);
                            let storage_key = format!("workspace:{}", workspace_id);
                            if let Ok(Some(state)) = storage.load_doc(&storage_key) {
                                let state_b64 = base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    &state,
                                );
                                let crdt_state = serde_json::json!({
                                    "type": "crdt_state",
                                    "state": state_b64
                                });
                                let sync_complete = serde_json::json!({
                                    "type": "sync_complete",
                                    "files_synced": files_synced
                                });
                                info!(
                                    "Completing handshake with CRDT state ({} bytes)",
                                    state.len()
                                );
                                return ControlMessageResponse::CompleteHandshake {
                                    responses: vec![
                                        crdt_state.to_string(),
                                        sync_complete.to_string(),
                                    ],
                                };
                            }

                            let sync_complete = serde_json::json!({
                                "type": "sync_complete",
                                "files_synced": files_synced
                            });
                            return ControlMessageResponse::CompleteHandshake {
                                responses: vec![sync_complete.to_string()],
                            };
                        }
                    }
                }

                let sync_complete = serde_json::json!({
                    "type": "sync_complete",
                    "files_synced": 0
                });
                ControlMessageResponse::CompleteHandshake {
                    responses: vec![sync_complete.to_string()],
                }
            }
            Some("focus") => {
                if let Some(files) = json.get("files").and_then(|v| v.as_array()) {
                    debug!("Client focusing on {} files", files.len());
                }
                ControlMessageResponse::Handled { responses: vec![] }
            }
            Some("unfocus") => {
                if let Some(files) = json.get("files").and_then(|v| v.as_array()) {
                    debug!("Client unfocusing {} files", files.len());
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
