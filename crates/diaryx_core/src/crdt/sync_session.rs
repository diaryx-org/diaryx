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
use super::sync::{
    DocIdKind, format_body_doc_id, format_workspace_doc_id, frame_message_v2, parse_doc_id,
    unframe_message_v2,
};
use super::sync_manager::RustSyncManager;
use super::sync_types::{SyncEvent, SyncSessionConfig, SyncStatus};
use crate::fs::AsyncFileSystem;
use crate::path_utils::normalize_sync_path;

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
        log::warn!(
            "[SyncSession] DEBUG mark_body_ready: path='{}', was_pending={}, remaining={}",
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
        log::warn!(
            "[SyncSession] DEBUG maybe_emit_synced: metadata_ready={}, pending_empty={}, pending_count={}, emitted={}, preview={:?}",
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

    fn queue_body_sync_step1_for_paths(
        &self,
        file_paths: &[String],
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

            for path in file_paths {
                let normalized = normalize_sync_path(path);
                if normalized.is_empty() {
                    continue;
                }
                if pending.contains(&normalized) || self.sync_manager.is_body_synced(&normalized) {
                    continue;
                }
                pending.insert(normalized.clone());
                docs_to_send.push(normalized);
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

        for (i, file_path) in docs_to_send.iter().enumerate() {
            let body_doc_id = format_body_doc_id(&self.config.workspace_id, file_path);
            let body_step1 = self.sync_manager.create_body_sync_step1(file_path);
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
    /// This is a lightweight self-heal pass that checks:
    /// - Active workspace files vs pending/synced body docs
    /// - Active workspace files vs on-disk presence (when write_to_disk=true)
    ///
    /// If drift is detected, it requeues body SyncStep1 for the affected files.
    async fn audit_and_reconcile_integrity(&self) -> Vec<SessionAction> {
        let mut actions = Vec::new();
        let active_paths = self.sync_manager.get_all_file_paths();
        if active_paths.is_empty() {
            self.pending_body_docs.lock().unwrap().clear();
            return actions;
        }

        let active_set: HashSet<String> = active_paths.iter().cloned().collect();
        {
            // Drop pending entries for files that are no longer active.
            let mut pending = self.pending_body_docs.lock().unwrap();
            pending.retain(|path| active_set.contains(path));
        }

        let mut unsynced_paths = Vec::new();
        let mut unloaded_paths = Vec::new();
        let mut missing_disk_paths = Vec::new();

        for path in &active_paths {
            if !self.sync_manager.is_body_synced(path) {
                unsynced_paths.push(path.clone());
            }
            if !self.sync_manager.is_body_loaded(path) {
                unloaded_paths.push(path.clone());
            }
            if self.config.write_to_disk && !self.sync_manager.file_exists_for_sync(path).await {
                missing_disk_paths.push(path.clone());
            }
        }

        if !missing_disk_paths.is_empty() || !unloaded_paths.is_empty() {
            log::warn!(
                "[SyncSession] Integrity audit: active={}, unsynced={}, unloaded={}, missing_disk={}",
                active_paths.len(),
                unsynced_paths.len(),
                unloaded_paths.len(),
                missing_disk_paths.len()
            );
        }

        if !missing_disk_paths.is_empty() {
            // Force rebootstrap for missing files by clearing prior synced status.
            for path in &missing_disk_paths {
                self.sync_manager.close_body_sync(path);
            }
            let mut heal_actions =
                self.queue_body_sync_step1_for_paths(&missing_disk_paths, false, false);
            actions.append(&mut heal_actions);
        }

        if !unsynced_paths.is_empty() {
            let mut sync_actions =
                self.queue_body_sync_step1_for_paths(&unsynced_paths, false, false);
            actions.append(&mut sync_actions);
        }

        actions
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

        if current_state != SessionState::Active {
            log::debug!("[SyncSession] Dropping local update (not active)");
            return Vec::new();
        }

        let framed = frame_message_v2(doc_id, data);
        vec![SessionAction::SendBinary(framed)]
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
                        if !result.changed_files.is_empty() {
                            log::debug!(
                                "[SyncSession] Workspace files changed: {:?}",
                                result.changed_files
                            );
                            actions.push(SessionAction::Emit(SyncEvent::FilesChanged {
                                files: result.changed_files,
                            }));
                        }

                        // Ensure body sync is started for newly discovered files after
                        // workspace metadata is applied.
                        let file_paths = self.sync_manager.get_all_file_paths();
                        let mut body_actions =
                            self.queue_body_sync_step1_for_paths(&file_paths, false, false);
                        actions.append(&mut body_actions);

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
            Some(DocIdKind::Body { file_path, .. }) => {
                let normalized_file_path = normalize_sync_path(&file_path);
                match self
                    .sync_manager
                    .handle_body_message(&normalized_file_path, &payload, self.config.write_to_disk)
                    .await
                {
                    Ok(result) => {
                        self.mark_body_ready(&normalized_file_path);
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            actions.push(SessionAction::SendBinary(framed));
                        }
                        if result.content.is_some() && !result.is_echo {
                            log::debug!("[SyncSession] Body changed: {}", normalized_file_path);
                            actions.push(SessionAction::Emit(SyncEvent::BodyChanged {
                                file_path: normalized_file_path.clone(),
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
                            normalized_file_path,
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
                if let Some(synced) = self.maybe_emit_synced() {
                    actions.push(synced);
                }
            }
            ControlMessage::PeerJoined { peer_count } => {
                log::info!("[SyncSession] Peer joined ({} connected)", peer_count);
            }
            ControlMessage::PeerLeft { peer_count } => {
                log::info!("[SyncSession] Peer left ({} connected)", peer_count);
            }
            ControlMessage::FocusListChanged { files } => {
                if !files.is_empty() {
                    log::debug!("[SyncSession] Focus list changed: {} files", files.len());
                }
            }
            _ => {}
        }

        actions
    }

    // =========================================================================
    // State Transitions
    // =========================================================================

    /// Transition to Active state: emit Syncing, send body SyncStep1 for all files.
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

        // Send body SyncStep1 for all known files
        let file_paths = self.sync_manager.get_all_file_paths();
        log::warn!(
            "[SyncSession] DEBUG transition_to_active: {} file paths to sync: {:?}",
            file_paths.len(),
            file_paths.iter().take(10).collect::<Vec<_>>()
        );
        let mut body_actions = self.queue_body_sync_step1_for_paths(&file_paths, true, true);
        actions.append(&mut body_actions);

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
    use crate::crdt::storage::CrdtStorage;
    use crate::crdt::{
        BodyDocManager, DocIdKind, MemoryStorage, SyncHandler, SyncMessage, WorkspaceCrdt,
    };
    use crate::fs::SyncToAsyncFs;
    use crate::test_utils::MockFileSystem;
    use futures_lite::future::block_on;

    type TestFs = SyncToAsyncFs<MockFileSystem>;

    fn test_file_metadata(filename: &str, title: &str) -> crate::crdt::FileMetadata {
        crate::crdt::FileMetadata::with_filename(filename.to_string(), Some(title.to_string()))
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
        let fs = SyncToAsyncFs::new(MockFileSystem::new());
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
            let Some(DocIdKind::Body { file_path, .. }) = parse_doc_id(&doc_id) else {
                continue;
            };
            let Ok(messages) = SyncMessage::decode_all(&payload) else {
                continue;
            };
            if messages
                .iter()
                .any(|msg| matches!(msg, SyncMessage::SyncStep1(_)))
            {
                targets.push(crate::path_utils::normalize_sync_path(&file_path));
            }
        }

        targets
    }

    #[test]
    fn test_join_bootstrap_dedupes_aliases_and_skips_temp_files() {
        let (session, _manager, workspace) = create_test_session("ws-join", false);

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
        workspace
            .set_file(
                "notes/new-entry.md.tmp",
                test_file_metadata("new-entry.md.tmp", "Temp"),
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
    fn test_workspace_update_during_join_queues_new_body_sync() {
        let (session, _manager, _workspace) = create_test_session("ws-rename", false);

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

        let actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-rename", SyncMessage::Update(update)),
        )));
        let targets = body_sync_step1_targets(&actions);

        assert!(targets.contains(&"renamed.md".to_string()));
    }

    #[test]
    fn test_reconnect_requeues_body_bootstrap_without_duplicates() {
        let (session, _manager, workspace) = create_test_session("ws-reconnect", false);
        workspace
            .set_file(
                "reconnect.md",
                test_file_metadata("reconnect.md", "Reconnect"),
            )
            .unwrap();

        block_on(session.process(IncomingEvent::Connected));
        let first_actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-reconnect", SyncMessage::SyncStep1(vec![])),
        )));
        let first_targets = body_sync_step1_targets(&first_actions);
        assert_eq!(first_targets, vec!["reconnect.md"]);

        block_on(session.process(IncomingEvent::Disconnected));
        block_on(session.process(IncomingEvent::Connected));
        let second_actions = block_on(session.process(IncomingEvent::BinaryMessage(
            framed_workspace_message("ws-reconnect", SyncMessage::SyncStep1(vec![])),
        )));
        let second_targets = body_sync_step1_targets(&second_actions);
        assert_eq!(second_targets, vec!["reconnect.md"]);
    }

    #[test]
    fn test_integrity_audit_requeues_when_disk_missing() {
        let (session, manager, workspace) = create_test_session("ws-heal", true);
        workspace
            .set_file("heal.md", test_file_metadata("heal.md", "Heal"))
            .unwrap();

        // Mark body as synced without writing to disk.
        let msg = SyncMessage::SyncStep1(vec![]).encode();
        block_on(manager.handle_body_message("heal.md", &msg, false)).unwrap();
        assert!(manager.is_body_synced("heal.md"));
        assert!(!block_on(manager.file_exists_for_sync("heal.md")));

        let actions = block_on(session.audit_and_reconcile_integrity());
        let targets = body_sync_step1_targets(&actions);

        assert!(targets.contains(&"heal.md".to_string()));
        assert!(!manager.is_body_synced("heal.md"));
    }
}
