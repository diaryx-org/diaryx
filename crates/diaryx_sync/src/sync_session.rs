//! Message-driven sync session protocol handler.
//!
//! `SyncSession` encapsulates the sync protocol logic (handshake, message
//! routing, framing, control messages) in a platform-agnostic way. Both
//! native `SyncClient` (tokio) and WASM `WasmSyncClient` (JS bridge)
//! delegate to this shared handler.
//!
//! # Architecture
//!
//! ```text
//!      Platform-specific layer
//!      ┌──────────────┬──────────────┐
//!      │ SyncClient   │ WasmSyncCli  │  ← owns transport, reconnection
//!      │ (tokio)      │ (JS bridge)  │
//!      └──────┬───────┴──────┬───────┘
//!             │              │
//!             └──────┬───────┘
//!                    ▼
//!           ┌──────────────────┐
//!           │   SyncSession    │  ← message-driven protocol handler
//!           │  (diaryx_core)   │    handshake, routing, framing
//!           └────────┬─────────┘
//!                    ▼
//!           ┌──────────────────┐
//!           │ RustSyncManager  │  ← Y-sync protocol, CRDT operations
//!           └──────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let session = SyncSession::new(config, sync_manager);
//!
//! // When connected:
//! let actions = session.process(IncomingEvent::Connected).await;
//! for action in actions {
//!     match action {
//!         SessionAction::SendBinary(data) => transport.send(data),
//!         SessionAction::Emit(event) => handler.on_event(event),
//!         // ...
//!     }
//! }
//! ```

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use base64::Engine;

use super::control_message::ControlMessage;
use super::sync_manager::RustSyncManager;
use super::sync_protocol::{
    DocIdKind, format_body_doc_id, format_workspace_doc_id, frame_message_v2, parse_doc_id,
    unframe_message_v2,
};
use super::sync_types::{SyncEvent, SyncSessionConfig, SyncStatus};
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::path_utils::normalize_sync_path;

/// Internal state machine for the handshake protocol.
#[derive(Debug, Clone, PartialEq)]
enum SessionState {
    /// Waiting for `Connected` event.
    AwaitingConnect,
    /// Connected, sent workspace SyncStep1, waiting for FileManifest / CrdtState / binary.
    WaitingForHandshake,
    /// Handshake complete, body SyncStep1s sent, active sync.
    Active,
}

/// Events fed into the session from the platform layer.
#[derive(Debug)]
pub enum IncomingEvent {
    /// WebSocket connected — triggers workspace SyncStep1 + handshake.
    Connected,
    /// Received a binary WebSocket message.
    BinaryMessage(Vec<u8>),
    /// Received a text WebSocket message (JSON control message).
    TextMessage(String),
    /// Snapshot was downloaded and imported (by the platform layer).
    SnapshotImported,
    /// WebSocket disconnected.
    Disconnected,
    /// A local CRDT update that needs to be sent to the server.
    LocalUpdate {
        /// The document ID (already formatted, e.g., "workspace:xxx" or "body:xxx/path").
        doc_id: String,
        /// The raw sync message bytes.
        data: Vec<u8>,
    },
    /// Request body sync for specific files.
    SyncBodyFiles {
        /// File paths to sync body docs for.
        file_paths: Vec<String>,
    },
}

/// Actions returned by `SyncSession::process()` for the platform layer to execute.
#[derive(Debug)]
pub enum SessionAction {
    /// Send binary data over the WebSocket.
    SendBinary(Vec<u8>),
    /// Send text data over the WebSocket.
    SendText(String),
    /// Download a workspace snapshot via HTTP, then call `SnapshotImported`.
    DownloadSnapshot {
        /// The workspace ID to download.
        workspace_id: String,
    },
    /// Emit a sync event to the UI.
    Emit(SyncEvent),
}

/// Message-driven sync session protocol handler.
///
/// Encapsulates the full sync protocol: handshake, binary message routing,
/// control message parsing, body SyncStep1 loop. Platform-agnostic — works
/// on both native (tokio) and WASM (single-threaded).
pub struct SyncSession<FS: AsyncFileSystem> {
    sync_manager: Arc<RustSyncManager<FS>>,
    config: SyncSessionConfig,
    state: Mutex<SessionState>,
    metadata_ready: Mutex<bool>,
    pending_body_docs: Mutex<HashSet<String>>,
    pending_handshake_updates: Mutex<Vec<(String, Vec<u8>)>>,
    synced_emitted: Mutex<bool>,
}

impl<FS: AsyncFileSystem> SyncSession<FS> {
    /// Create a new sync session.
    pub fn new(config: SyncSessionConfig, sync_manager: Arc<RustSyncManager<FS>>) -> Self {
        Self {
            sync_manager,
            config,
            state: Mutex::new(SessionState::AwaitingConnect),
            metadata_ready: Mutex::new(false),
            pending_body_docs: Mutex::new(HashSet::new()),
            pending_handshake_updates: Mutex::new(Vec::new()),
            synced_emitted: Mutex::new(false),
        }
    }

    fn set_metadata_ready(&self) {
        let mut ready = self.metadata_ready.lock().unwrap();
        *ready = true;
    }

    fn mark_body_ready(&self, path: &str) {
        let normalized = normalize_sync_path(path);
        let mut pending = self.pending_body_docs.lock().unwrap();
        let was_present = pending.remove(&normalized);
        log::debug!(
            "[SyncSession] mark_body_ready: path='{}', was_pending={}, remaining={}",
            normalized,
            was_present,
            pending.len()
        );
    }

    fn maybe_emit_synced(&self) -> Option<SessionAction> {
        let metadata_ready = *self.metadata_ready.lock().unwrap();
        let pending = self.pending_body_docs.lock().unwrap();
        let pending_empty = pending.is_empty();
        let pending_count = pending.len();
        let pending_preview: Vec<_> = pending.iter().take(5).cloned().collect();
        drop(pending);
        let mut emitted = self.synced_emitted.lock().unwrap();
        log::debug!(
            "[SyncSession] maybe_emit_synced: metadata_ready={}, pending_empty={}, pending_count={}, emitted={}, preview={:?}",
            metadata_ready,
            pending_empty,
            pending_count,
            *emitted,
            pending_preview
        );
        if metadata_ready && pending_empty && !*emitted {
            *emitted = true;
            return Some(SessionAction::Emit(SyncEvent::StatusChanged {
                status: SyncStatus::Synced,
            }));
        }
        None
    }

    fn queue_handshake_update(&self, doc_id: &str, data: &[u8]) {
        self.pending_handshake_updates
            .lock()
            .unwrap()
            .push((doc_id.to_string(), data.to_vec()));
    }

    fn drain_handshake_updates(&self) -> Vec<SessionAction> {
        let pending = std::mem::take(&mut *self.pending_handshake_updates.lock().unwrap());
        pending
            .into_iter()
            .map(|(doc_id, data)| SessionAction::SendBinary(frame_message_v2(&doc_id, &data)))
            .collect()
    }

    /// Queue body SyncStep1 messages for the given UUIDs.
    fn queue_body_sync_step1(
        &self,
        uuids: &[String],
        reset_pending: bool,
        emit_initial_progress: bool,
    ) -> Vec<SessionAction> {
        let mut actions = Vec::new();
        let mut docs_to_send: Vec<String> = Vec::new();

        {
            let mut pending = self.pending_body_docs.lock().unwrap();
            if reset_pending {
                pending.clear();
            }

            for uuid in uuids {
                if uuid.is_empty() {
                    continue;
                }
                if pending.contains(uuid) || self.sync_manager.is_body_synced(uuid) {
                    continue;
                }
                pending.insert(uuid.clone());
                docs_to_send.push(uuid.clone());
            }
        }

        if docs_to_send.is_empty() {
            return actions;
        }

        if emit_initial_progress {
            actions.push(SessionAction::Emit(SyncEvent::Progress {
                completed: 0,
                total: docs_to_send.len(),
            }));
        }

        for (i, uuid) in docs_to_send.iter().enumerate() {
            let body_doc_id = format_body_doc_id(&self.config.workspace_id, uuid);
            let body_step1 = self.sync_manager.create_body_sync_step1(uuid);
            let body_framed = frame_message_v2(&body_doc_id, &body_step1);
            actions.push(SessionAction::SendBinary(body_framed));

            if emit_initial_progress && (i + 1) % 50 == 0 {
                actions.push(SessionAction::Emit(SyncEvent::Progress {
                    completed: i + 1,
                    total: docs_to_send.len(),
                }));
            }
        }

        log::info!(
            "[SyncSession] Sent body SyncStep1 for {} files",
            docs_to_send.len()
        );
        actions
    }

    /// Audit sync integrity and requeue missing body syncs when needed.
    ///
    /// This is a lightweight self-heal pass that checks focused files:
    /// - Focused files vs pending/synced body docs
    /// - Focused files vs on-disk presence (when write_to_disk=true)
    ///
    /// If drift is detected, it requeues body SyncStep1 for the affected files.
    /// Only audits focused files to avoid eagerly syncing all body docs.
    async fn audit_and_reconcile_integrity(&self) -> Vec<SessionAction> {
        let mut actions = Vec::new();
        let focused_paths = self.sync_manager.get_focused_files();
        if focused_paths.is_empty() {
            return actions;
        }

        // Verify focused files are still in the workspace (resolve to UUIDs)
        let active_uuids: HashSet<String> =
            self.sync_manager.get_all_file_uuids().into_iter().collect();

        // Resolve focused paths to UUIDs
        let focused_uuids: Vec<String> = focused_paths
            .into_iter()
            .filter_map(|p| self.sync_manager.resolve_uuid(&p))
            .filter(|uuid| active_uuids.contains(uuid))
            .collect();

        {
            // Drop pending entries for files that are no longer active.
            let mut pending = self.pending_body_docs.lock().unwrap();
            pending.retain(|uuid| active_uuids.contains(uuid));
        }

        let mut unsynced_uuids = Vec::new();
        let mut unloaded_uuids = Vec::new();
        let mut missing_disk_uuids = Vec::new();

        for uuid in &focused_uuids {
            if !self.sync_manager.is_body_synced(uuid) {
                unsynced_uuids.push(uuid.clone());
            }
            if !self.sync_manager.is_body_loaded(uuid) {
                unloaded_uuids.push(uuid.clone());
            }
            if self.config.write_to_disk && !self.focused_body_exists_on_disk(uuid).await {
                missing_disk_uuids.push(uuid.clone());
            }
        }

        if !missing_disk_uuids.is_empty() || !unloaded_uuids.is_empty() {
            log::warn!(
                "[SyncSession] Integrity audit: focused={}, unsynced={}, unloaded={}, missing_disk={}",
                focused_uuids.len(),
                unsynced_uuids.len(),
                unloaded_uuids.len(),
                missing_disk_uuids.len()
            );
        }

        if !missing_disk_uuids.is_empty() {
            // Force rebootstrap for missing files by clearing prior synced status.
            for uuid in &missing_disk_uuids {
                self.sync_manager.close_body_sync(uuid);
            }
            let mut heal_actions = self.queue_body_sync_step1(&missing_disk_uuids, false, false);
            actions.append(&mut heal_actions);
        }

        if !unsynced_uuids.is_empty() {
            let mut sync_actions = self.queue_body_sync_step1(&unsynced_uuids, false, false);
            actions.append(&mut sync_actions);
        }

        actions
    }

    async fn focused_body_exists_on_disk(&self, uuid: &str) -> bool {
        let uuid = normalize_sync_path(uuid);
        let resolved_path = self
            .sync_manager
            .resolve_path(&uuid)
            .map(|path| normalize_sync_path(&path));

        if let Some(path) = resolved_path.as_ref()
            && self.sync_manager.file_exists_for_sync(path).await
        {
            return true;
        }

        // Browser-backed workspaces can keep the actual file at the raw CRDT
        // key even when the workspace tree derives a nested display path.
        if resolved_path.as_deref() != Some(uuid.as_str())
            && self.sync_manager.file_exists_for_sync(&uuid).await
        {
            return true;
        }

        false
    }

    /// Process an incoming event and return actions for the platform layer.
    ///
    /// This is the main entry point. The platform layer feeds events (connected,
    /// messages, disconnected) and executes the returned actions (send data, emit
    /// events, download snapshots).
    pub async fn process(&self, event: IncomingEvent) -> Vec<SessionAction> {
        match event {
            IncomingEvent::Connected => self.handle_connected().await,
            IncomingEvent::BinaryMessage(data) => self.handle_binary_message(&data).await,
            IncomingEvent::TextMessage(text) => self.handle_text_message(&text).await,
            IncomingEvent::SnapshotImported => self.handle_snapshot_imported().await,
            IncomingEvent::Disconnected => self.handle_disconnected(),
            IncomingEvent::LocalUpdate { doc_id, data } => self.handle_local_update(&doc_id, &data),
            IncomingEvent::SyncBodyFiles { file_paths } => {
                self.handle_sync_body_files(&file_paths).await
            }
        }
    }

    /// Reset session state (e.g., on disconnect before reconnect).
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        *state = SessionState::AwaitingConnect;
        *self.metadata_ready.lock().unwrap() = false;
        self.pending_body_docs.lock().unwrap().clear();
        *self.synced_emitted.lock().unwrap() = false;
    }

    // =========================================================================
    // Event Handlers
    // =========================================================================

    async fn handle_connected(&self) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        // Update state
        {
            let mut state = self.state.lock().unwrap();
            *state = SessionState::WaitingForHandshake;
        }
        *self.metadata_ready.lock().unwrap() = false;
        self.pending_body_docs.lock().unwrap().clear();
        *self.synced_emitted.lock().unwrap() = false;

        // Send workspace SyncStep1 (framed v2)
        let ws_doc_id = format_workspace_doc_id(&self.config.workspace_id);
        let ws_step1 = self.sync_manager.create_workspace_sync_step1();
        let ws_framed = frame_message_v2(&ws_doc_id, &ws_step1);
        actions.push(SessionAction::SendBinary(ws_framed));

        actions.push(SessionAction::Emit(SyncEvent::StatusChanged {
            status: SyncStatus::Connected,
        }));

        actions
    }

    async fn handle_binary_message(&self, data: &[u8]) -> Vec<SessionAction> {
        let current_state = {
            let state = self.state.lock().unwrap();
            state.clone()
        };

        match current_state {
            SessionState::WaitingForHandshake => {
                // Server returned binary during handshake — no handshake needed,
                // transition to active and process the message.
                let mut actions = self.transition_to_active().await;
                let mut routed = self.route_binary_message(data).await;
                actions.append(&mut routed);
                let mut heal_actions = self.audit_and_reconcile_integrity().await;
                actions.append(&mut heal_actions);
                if let Some(synced) = self.maybe_emit_synced() {
                    actions.push(synced);
                }
                actions
            }
            SessionState::Active => self.route_binary_message(data).await,
            SessionState::AwaitingConnect => {
                log::warn!("[SyncSession] Binary message received before connect");
                Vec::new()
            }
        }
    }

    async fn handle_text_message(&self, text: &str) -> Vec<SessionAction> {
        let current_state = {
            let state = self.state.lock().unwrap();
            state.clone()
        };

        match current_state {
            SessionState::WaitingForHandshake => self.handle_handshake_message(text).await,
            SessionState::Active => self.handle_control_message(text).await,
            SessionState::AwaitingConnect => {
                log::warn!("[SyncSession] Text message received before connect");
                Vec::new()
            }
        }
    }

    async fn handle_snapshot_imported(&self) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        // After snapshot import, send FilesReady to continue handshake
        actions.push(SessionAction::SendText(
            r#"{"type":"FilesReady"}"#.to_string(),
        ));

        log::info!("[SyncSession] Snapshot imported, sent FilesReady");
        actions
    }

    fn handle_disconnected(&self) -> Vec<SessionAction> {
        // Reset state
        {
            let mut state = self.state.lock().unwrap();
            *state = SessionState::AwaitingConnect;
        }
        *self.metadata_ready.lock().unwrap() = false;
        self.pending_body_docs.lock().unwrap().clear();
        self.pending_handshake_updates.lock().unwrap().clear();
        *self.synced_emitted.lock().unwrap() = false;

        // Reset sync manager state
        self.sync_manager.reset();

        vec![SessionAction::Emit(SyncEvent::StatusChanged {
            status: SyncStatus::Disconnected,
        })]
    }

    fn handle_local_update(&self, doc_id: &str, data: &[u8]) -> Vec<SessionAction> {
        let current_state = {
            let state = self.state.lock().unwrap();
            state.clone()
        };

        match current_state {
            SessionState::Active => {
                let framed = frame_message_v2(doc_id, data);
                vec![SessionAction::SendBinary(framed)]
            }
            SessionState::WaitingForHandshake => {
                log::debug!("[SyncSession] Buffering local update during handshake");
                self.queue_handshake_update(doc_id, data);
                Vec::new()
            }
            SessionState::AwaitingConnect => {
                log::debug!("[SyncSession] Dropping local update (awaiting connect)");
                Vec::new()
            }
        }
    }

    async fn handle_sync_body_files(&self, file_paths: &[String]) -> Vec<SessionAction> {
        let current_state = {
            let state = self.state.lock().unwrap();
            state.clone()
        };

        if current_state != SessionState::Active {
            log::debug!("[SyncSession] Dropping SyncBodyFiles (not active)");
            return Vec::new();
        }

        self.sync_manager.set_focused_files(file_paths);

        // Resolve paths to UUIDs for body sync.
        // file_paths can contain either UUIDs (already resolved) or filesystem paths.
        let uuids: Vec<String> = file_paths
            .iter()
            .filter_map(|path| {
                // If it looks like a UUID already, use it directly
                if !path.contains('/') && !path.contains('.') {
                    return Some(path.clone());
                }
                self.sync_manager.resolve_uuid(path)
            })
            .collect();

        // Load body content from disk before syncing so that files opened
        // for the first time have their content available in the CRDT.
        // Skip files that already have a pending SyncStep1 — the server will
        // provide their content via SyncStep2. Loading from disk in that
        // window would create duplicate CRDT operations with a new client ID,
        // causing content duplication when the server's response arrives.
        for uuid in &uuids {
            let is_pending = self.pending_body_docs.lock().unwrap().contains(uuid);
            if !is_pending {
                let _ = self.sync_manager.ensure_body_content_loaded(uuid).await;
            }
        }

        // Clear body_synced for requested files so queue_body_sync_step1 will
        // re-subscribe via SyncStep1. After the initial bootstrap sync, the server
        // may stop forwarding incremental updates (siphonophore subscription lifecycle),
        // so an explicit SyncBodyFiles request must force a fresh SyncStep1 to get
        // the latest state via SyncStep2.
        for uuid in &uuids {
            self.sync_manager.clear_body_synced(uuid);
        }

        log::info!(
            "[SyncSession] SyncBodyFiles: {} files requested, {} resolved to UUIDs",
            file_paths.len(),
            uuids.len()
        );
        self.queue_body_sync_step1(&uuids, false, false)
    }

    // =========================================================================
    // Handshake Protocol
    // =========================================================================

    async fn handle_handshake_message(&self, text: &str) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        let ctrl = match serde_json::from_str::<ControlMessage>(text) {
            Ok(ctrl) => ctrl,
            Err(_) => {
                log::debug!("[SyncSession] Unrecognized text during handshake: {}", text);
                return actions;
            }
        };

        match ctrl {
            ControlMessage::FileManifest { client_is_new, .. } => {
                if client_is_new {
                    // New client — platform layer should download snapshot
                    log::info!(
                        "[SyncSession] FileManifest: client_is_new=true, requesting snapshot download"
                    );
                    actions.push(SessionAction::DownloadSnapshot {
                        workspace_id: self.config.workspace_id.clone(),
                    });
                } else {
                    // Existing client — send FilesReady immediately
                    log::info!(
                        "[SyncSession] FileManifest: client_is_new=false, sending FilesReady"
                    );
                    actions.push(SessionAction::SendText(
                        r#"{"type":"FilesReady"}"#.to_string(),
                    ));
                }
            }
            ControlMessage::CrdtState { state } => {
                match base64::engine::general_purpose::STANDARD.decode(&state) {
                    Ok(state_bytes) => {
                        match self.sync_manager.handle_crdt_state(&state_bytes).await {
                            Ok(count) => {
                                log::info!("[SyncSession] Applied CRDT state ({} files)", count);
                                actions.push(SessionAction::Emit(SyncEvent::FilesChanged {
                                    files: vec![],
                                }));
                                self.set_metadata_ready();
                            }
                            Err(e) => {
                                log::error!("[SyncSession] Failed to apply CRDT state: {}", e);
                                actions.push(SessionAction::Emit(SyncEvent::Error {
                                    message: format!("Failed to apply CRDT state: {}", e),
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("[SyncSession] Failed to decode CRDT state: {}", e);
                        actions.push(SessionAction::Emit(SyncEvent::Error {
                            message: format!("Failed to decode CRDT state: {}", e),
                        }));
                    }
                }

                // Handshake complete — transition to active
                let mut active_actions = self.transition_to_active().await;
                actions.append(&mut active_actions);
                let mut heal_actions = self.audit_and_reconcile_integrity().await;
                actions.append(&mut heal_actions);
                if let Some(synced) = self.maybe_emit_synced() {
                    actions.push(synced);
                }
            }
            ControlMessage::SessionJoined { .. } => {
                log::info!("[SyncSession] Session joined during handshake");
            }
            ControlMessage::SyncComplete { .. } => {
                self.set_metadata_ready();
            }
            _ => {
                log::debug!("[SyncSession] Ignoring {:?} during handshake", ctrl);
            }
        }

        actions
    }

    // =========================================================================
    // Active State — Message Routing
    // =========================================================================

    async fn route_binary_message(&self, data: &[u8]) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        let (doc_id, payload) = match unframe_message_v2(data) {
            Some(pair) => pair,
            None => {
                log::debug!("[SyncSession] Failed to unframe binary message");
                return actions;
            }
        };

        match parse_doc_id(&doc_id) {
            Some(DocIdKind::Workspace(_)) => {
                match self
                    .sync_manager
                    .handle_workspace_message(&payload, self.config.write_to_disk)
                    .await
                {
                    Ok(result) => {
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            actions.push(SessionAction::SendBinary(framed));
                        }
                        if result.sync_complete {
                            self.set_metadata_ready();
                        }
                        let changed_files = result.changed_files.clone();
                        if !result.changed_files.is_empty() {
                            log::debug!(
                                "[SyncSession] Workspace files changed: {:?}",
                                result.changed_files
                            );
                            actions.push(SessionAction::Emit(SyncEvent::FilesChanged {
                                files: changed_files.clone(),
                            }));
                        }

                        let mut heal_actions = self.audit_and_reconcile_integrity().await;
                        actions.append(&mut heal_actions);

                        if let Some(synced) = self.maybe_emit_synced() {
                            actions.push(synced);
                        }
                    }
                    Err(e) => {
                        log::error!("[SyncSession] Error handling workspace message: {}", e);
                        actions.push(SessionAction::Emit(SyncEvent::Error {
                            message: e.to_string(),
                        }));
                    }
                }
            }
            Some(DocIdKind::Body { body_id, .. }) => {
                let body_is_active = self.sync_manager.resolve_path(&body_id).is_some();
                let body_is_focused = self.sync_manager.is_file_focused(&body_id);
                if !body_is_active && !body_is_focused {
                    log::debug!(
                        "[SyncSession] Ignoring stale body message for inactive doc_id={}",
                        body_id
                    );
                    return actions;
                }
                match self
                    .sync_manager
                    .handle_body_message(&body_id, &payload, self.config.write_to_disk)
                    .await
                {
                    Ok(result) => {
                        self.mark_body_ready(&body_id);
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            actions.push(SessionAction::SendBinary(framed));
                        }
                        if let Some(content) = result.content
                            && !result.is_echo
                        {
                            // Resolve UUID to path for the UI event
                            let resolved_path = self
                                .sync_manager
                                .resolve_path(&body_id)
                                .unwrap_or_else(|| body_id.clone());
                            log::debug!(
                                "[SyncSession] Body changed: {} ({})",
                                resolved_path,
                                body_id
                            );
                            actions.push(SessionAction::Emit(SyncEvent::BodyChanged {
                                file_path: resolved_path,
                                body: content,
                            }));
                        }
                        let mut heal_actions = self.audit_and_reconcile_integrity().await;
                        actions.append(&mut heal_actions);
                        if let Some(synced) = self.maybe_emit_synced() {
                            actions.push(synced);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "[SyncSession] Error handling body message for {}: {}",
                            body_id,
                            e
                        );
                    }
                }
            }
            None => {
                log::debug!("[SyncSession] Unknown doc_id: {}", doc_id);
            }
        }

        actions
    }

    async fn handle_control_message(&self, text: &str) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        let ctrl = match serde_json::from_str::<ControlMessage>(text) {
            Ok(ctrl) => ctrl,
            Err(_) => return actions,
        };

        match ctrl {
            ControlMessage::SyncProgress { completed, total } => {
                log::debug!("[SyncSession] Progress: {}/{}", completed, total);
                actions.push(SessionAction::Emit(SyncEvent::Progress {
                    completed,
                    total,
                }));
            }
            ControlMessage::SyncComplete { files_synced } => {
                log::info!("[SyncSession] Sync complete ({} files)", files_synced);
                self.set_metadata_ready();
                let mut heal_actions = self.audit_and_reconcile_integrity().await;
                actions.append(&mut heal_actions);
                actions.push(SessionAction::Emit(SyncEvent::SyncComplete {
                    files_synced,
                }));
                if let Some(synced) = self.maybe_emit_synced() {
                    actions.push(synced);
                }
            }
            ControlMessage::PeerJoined { peer_count } => {
                log::info!("[SyncSession] Peer joined ({} connected)", peer_count);
                actions.push(SessionAction::Emit(SyncEvent::PeerJoined { peer_count }));
            }
            ControlMessage::PeerLeft { peer_count } => {
                log::info!("[SyncSession] Peer left ({} connected)", peer_count);
                actions.push(SessionAction::Emit(SyncEvent::PeerLeft { peer_count }));
            }
            ControlMessage::FocusListChanged { ref files } => {
                if !files.is_empty() {
                    log::debug!("[SyncSession] Focus list changed: {} files", files.len());
                }
                actions.push(SessionAction::Emit(SyncEvent::FocusListChanged {
                    files: files.clone(),
                }));
            }
            _ => {}
        }

        actions
    }

    // =========================================================================
    // State Transitions
    // =========================================================================

    /// Transition to Active state: emit Syncing, set metadata ready, and bootstrap body sync.
    async fn transition_to_active(&self) -> Vec<SessionAction> {
        let mut actions = Vec::new();

        {
            let mut state = self.state.lock().unwrap();
            *state = SessionState::Active;
        }

        actions.push(SessionAction::Emit(SyncEvent::StatusChanged {
            status: SyncStatus::Syncing,
        }));

        // Compatibility fallback for servers that don't emit explicit sync_complete.
        self.set_metadata_ready();

        let all_uuids = self.sync_manager.get_all_file_uuids();

        // Load body content from disk for all files before syncing.
        // Without this, body docs start empty and the SyncStep1 sends an empty
        // state vector, meaning files that are never edited (like the root index)
        // would never have their content uploaded.
        let mut loaded_count = 0usize;
        for uuid in &all_uuids {
            match self.sync_manager.ensure_body_content_loaded(uuid).await {
                Ok(true) => loaded_count += 1,
                Ok(false) => {}
                Err(e) => {
                    log::warn!(
                        "[SyncSession] Failed to load body content for {}: {:?}",
                        uuid,
                        e
                    );
                }
            }
        }
        if loaded_count > 0 {
            log::info!(
                "[SyncSession] Loaded body content from disk for {} files",
                loaded_count
            );
        }

        let mut body_bootstrap =
            self.queue_body_sync_step1(&all_uuids, true, !all_uuids.is_empty());
        actions.append(&mut body_bootstrap);

        let mut pending_updates = self.drain_handshake_updates();
        actions.append(&mut pending_updates);

        log::info!(
            "[SyncSession] transition_to_active: {} files in workspace (body bootstrap queued)",
            all_uuids.len()
        );

        actions
    }
}

impl<FS: AsyncFileSystem> std::fmt::Debug for SyncSession<FS> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().unwrap();
        f.debug_struct("SyncSession")
            .field("workspace_id", &self.config.workspace_id)
            .field("state", &*state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt_storage::CrdtStorage;
    use crate::{
        BodyDocManager, DocIdKind, MemoryStorage, SyncHandler, SyncMessage, WorkspaceCrdt,
    };
    use diaryx_core::fs::{AsyncFileSystem, InMemoryFileSystem, SyncToAsyncFs};
    use futures_lite::future::block_on;
    use std::path::Path;

    type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

    fn test_file_metadata(filename: &str, title: &str) -> crate::FileMetadata {
        crate::FileMetadata::with_filename(filename.to_string(), Some(title.to_string()))
    }

    fn create_test_session(
        workspace_id: &str,
        write_to_disk: bool,
    ) -> (
        SyncSession<TestFs>,
        Arc<RustSyncManager<TestFs>>,
        Arc<WorkspaceCrdt>,
    ) {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let sync_handler = Arc::new(SyncHandler::new(fs));
        let sync_manager = Arc::new(RustSyncManager::new(
            Arc::clone(&workspace_crdt),
            body_manager,
            sync_handler,
        ));
        let session = SyncSession::new(
            SyncSessionConfig {
                workspace_id: workspace_id.to_string(),
                write_to_disk,
            },
            Arc::clone(&sync_manager),
        );

        (session, sync_manager, workspace_crdt)
    }

    fn create_test_session_with_fs(
        workspace_id: &str,
        write_to_disk: bool,
    ) -> (
        SyncSession<TestFs>,
        Arc<RustSyncManager<TestFs>>,
        Arc<WorkspaceCrdt>,
        TestFs,
    ) {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let sync_handler = Arc::new(SyncHandler::new(fs.clone()));
        let sync_manager = Arc::new(RustSyncManager::new(
            Arc::clone(&workspace_crdt),
            body_manager,
            sync_handler,
        ));
        let session = SyncSession::new(
            SyncSessionConfig {
                workspace_id: workspace_id.to_string(),
                write_to_disk,
            },
            Arc::clone(&sync_manager),
        );

        (session, sync_manager, workspace_crdt, fs)
    }

    fn framed_workspace_message(workspace_id: &str, message: SyncMessage) -> Vec<u8> {
        let doc_id = format_workspace_doc_id(workspace_id);
        let payload = message.encode();
        frame_message_v2(&doc_id, &payload)
    }

    fn body_sync_step1_targets(actions: &[SessionAction]) -> Vec<String> {
        let mut targets = Vec::new();

        for action in actions {
            let SessionAction::SendBinary(data) = action else {
                continue;
            };
            let Some((doc_id, payload)) = unframe_message_v2(data) else {
                continue;
            };
            let Some(DocIdKind::Body { body_id, .. }) = parse_doc_id(&doc_id) else {
                continue;
            };
            let Ok(messages) = SyncMessage::decode_all(&payload) else {
                continue;
            };
            if messages
                .iter()
                .any(|msg| matches!(msg, SyncMessage::SyncStep1(_)))
            {
                targets.push(body_id);
            }
        }

        targets
    }

    fn sent_payloads_for_doc(actions: &[SessionAction], expected_doc_id: &str) -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();

        for action in actions {
            let SessionAction::SendBinary(data) = action else {
                continue;
            };
            let Some((doc_id, payload)) = unframe_message_v2(data) else {
                continue;
            };
            if doc_id == expected_doc_id {
                payloads.push(payload);
            }
        }

        payloads
    }

    fn framed_body_message(workspace_id: &str, body_id: &str, message: SyncMessage) -> Vec<u8> {
        let doc_id = format_body_doc_id(workspace_id, body_id);
        let payload = message.encode();
        frame_message_v2(&doc_id, &payload)
    }

    #[test]
    fn test_transition_to_active_bootstraps_body_sync_for_workspace_files() {
        let (session, _manager, workspace) = create_test_session("ws-join", false);

        workspace
            .set_file("./README.md", test_file_metadata("README.md", "Readme"))
            .unwrap();
        workspace
            .set_file(
                "notes/new-entry.md",
                test_file_metadata("new-entry.md", "New Entry"),
            )
            .unwrap();

        block_on(session.process(IncomingEvent::Connected));
        let actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-join", SyncMessage::SyncStep1(vec![])),
        )));

        let mut targets = body_sync_step1_targets(&actions);
        targets.sort();
        assert_eq!(targets, vec!["README.md", "notes/new-entry.md"]);
    }

    #[test]
    fn test_local_updates_buffered_during_handshake_flush_on_activate() {
        let (session, _manager, _workspace) = create_test_session("ws-buffer", false);

        block_on(session.process(IncomingEvent::Connected));

        let local_doc_id = format_workspace_doc_id("ws-buffer");
        let local_payload = vec![1, 2, 3, 4];
        let buffered = block_on(session.process(IncomingEvent::LocalUpdate {
            doc_id: local_doc_id.clone(),
            data: local_payload.clone(),
        }));
        assert!(buffered.is_empty());

        let actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-buffer", SyncMessage::SyncStep1(vec![])),
        )));

        let sent_payloads = sent_payloads_for_doc(&actions, &local_doc_id);
        assert!(
            sent_payloads
                .iter()
                .any(|payload| payload == &local_payload),
            "expected buffered local payload to flush after activation, got {:?}",
            sent_payloads
        );
    }

    #[test]
    fn test_sync_body_files_skips_docs_already_pending_from_bootstrap() {
        let (session, _manager, workspace) = create_test_session("ws-body", false);

        workspace
            .set_file("./README.md", test_file_metadata("README.md", "Readme"))
            .unwrap();
        workspace
            .set_file("/README.md", test_file_metadata("README.md", "Readme"))
            .unwrap();
        workspace
            .set_file(
                "notes/new-entry.md",
                test_file_metadata("new-entry.md", "New Entry"),
            )
            .unwrap();

        // Enter active state
        block_on(session.process(IncomingEvent::Connected));
        block_on(
            session.process(IncomingEvent::BinaryMessage(framed_workspace_message(
                "ws-body",
                SyncMessage::SyncStep1(vec![]),
            ))),
        );

        // Request body sync for specific files
        let actions = block_on(session.process(IncomingEvent::SyncBodyFiles {
            file_paths: vec![
                "./README.md".to_string(),
                "/README.md".to_string(),
                "notes/new-entry.md".to_string(),
            ],
        }));

        // Eager bootstrap already queued these docs, so there should be no duplicates.
        let targets = body_sync_step1_targets(&actions);
        assert!(
            targets.is_empty(),
            "Expected no duplicates, got {:?}",
            targets
        );
    }

    #[test]
    fn test_sync_body_files_ignored_when_not_active() {
        let (session, _manager, workspace) = create_test_session("ws-inactive", false);

        workspace
            .set_file("test.md", test_file_metadata("test.md", "Test"))
            .unwrap();

        // Session is in AwaitingConnect state — SyncBodyFiles should be dropped
        let actions = block_on(session.process(IncomingEvent::SyncBodyFiles {
            file_paths: vec!["test.md".to_string()],
        }));
        let targets = body_sync_step1_targets(&actions);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_workspace_update_bootstraps_new_focused_body_docs_via_audit() {
        let (session, manager, workspace) = create_test_session("ws-rename", false);

        workspace
            .set_file("existing.md", test_file_metadata("existing.md", "Existing"))
            .unwrap();

        // Enter active sync state.
        block_on(session.process(IncomingEvent::Connected));
        block_on(
            session.process(IncomingEvent::BinaryMessage(framed_workspace_message(
                "ws-rename",
                SyncMessage::SyncStep1(vec![]),
            ))),
        );

        // Simulate metadata update arriving after initial transition to active.
        let source_storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let source_workspace = WorkspaceCrdt::new(Arc::clone(&source_storage));
        source_workspace
            .set_file("renamed.md", test_file_metadata("renamed.md", "Renamed"))
            .unwrap();
        let update = source_workspace.encode_state_as_update();

        manager.add_focused_files(&["renamed.md".to_string()]);
        manager.rebuild_uuid_maps();

        let actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-rename", SyncMessage::Update(update)),
        )));
        let mut targets = body_sync_step1_targets(&actions);
        targets.sort();

        assert_eq!(targets, vec!["renamed.md"]);
    }

    #[test]
    fn test_integrity_audit_only_checks_focused_files() {
        let (session, manager, workspace) = create_test_session("ws-heal", true);
        workspace
            .set_file("heal.md", test_file_metadata("heal.md", "Heal"))
            .unwrap();
        workspace
            .set_file("other.md", test_file_metadata("other.md", "Other"))
            .unwrap();

        // Rebuild UUID maps so resolve_uuid() works in audit_and_reconcile_integrity.
        manager.rebuild_uuid_maps();

        // Focus on heal.md only
        manager.add_focused_files(&["heal.md".to_string()]);

        // Mark body as synced without writing to disk.
        let msg = SyncMessage::SyncStep1(vec![]).encode();
        block_on(manager.handle_body_message("heal.md", &msg, false)).unwrap();
        assert!(manager.is_body_synced("heal.md"));
        assert!(!block_on(manager.file_exists_for_sync("heal.md")));

        let actions = block_on(session.audit_and_reconcile_integrity());
        let targets = body_sync_step1_targets(&actions);

        // Only heal.md should be requeued (focused), not other.md
        assert!(targets.contains(&"heal.md".to_string()));
        assert!(!targets.contains(&"other.md".to_string()));
        assert!(!manager.is_body_synced("heal.md"));
    }

    #[test]
    fn test_integrity_audit_empty_when_no_focused_files() {
        let (session, _manager, workspace) = create_test_session("ws-nofocus", true);
        workspace
            .set_file("file.md", test_file_metadata("file.md", "File"))
            .unwrap();

        // No files focused — audit should return empty
        let actions = block_on(session.audit_and_reconcile_integrity());
        assert!(actions.is_empty());
    }

    #[test]
    fn test_integrity_audit_accepts_flat_storage_alias_for_focused_move() {
        let (session, manager, workspace, fs) = create_test_session_with_fs("ws-flat", true);

        workspace
            .set_file("parent", test_file_metadata("parent", "Parent"))
            .unwrap();
        let mut child = test_file_metadata("child.md", "Child");
        child.part_of = Some("parent".to_string());
        workspace.set_file("child.md", child).unwrap();

        manager.rebuild_uuid_maps();
        manager.add_focused_files(&["parent/child.md".to_string()]);

        block_on(fs.write_file(
            Path::new("child.md"),
            "---\ntitle: Child\npart_of: parent\n---\nBody",
        ))
        .unwrap();

        let msg = SyncMessage::SyncStep1(vec![]).encode();
        block_on(manager.handle_body_message("child.md", &msg, false)).unwrap();
        assert!(manager.is_body_synced("child.md"));

        let actions = block_on(session.audit_and_reconcile_integrity());
        let targets = body_sync_step1_targets(&actions);
        assert!(
            targets.is_empty(),
            "expected no body resubscribe when raw doc-id storage exists, got {:?}",
            targets
        );
    }

    #[test]
    fn test_ignores_body_messages_for_inactive_doc_ids() {
        let (session, manager, workspace) = create_test_session("ws-stale", false);
        workspace
            .set_file("active.md", test_file_metadata("active.md", "Active"))
            .unwrap();
        manager.rebuild_uuid_maps();

        block_on(session.process(IncomingEvent::Connected));
        block_on(
            session.process(IncomingEvent::BinaryMessage(framed_workspace_message(
                "ws-stale",
                SyncMessage::SyncStep1(vec![]),
            ))),
        );

        let actions = block_on(
            session.process(IncomingEvent::BinaryMessage(framed_body_message(
                "ws-stale",
                "stale.md",
                SyncMessage::SyncStep1(vec![]),
            ))),
        );

        assert!(
            body_sync_step1_targets(&actions).is_empty(),
            "stale body messages should not trigger responses: {:?}",
            body_sync_step1_targets(&actions)
        );
        assert!(
            actions
                .iter()
                .all(|action| !matches!(action, SessionAction::SendBinary(_))),
            "stale body messages should not emit binary responses: {:?}",
            actions
        );
    }
}
