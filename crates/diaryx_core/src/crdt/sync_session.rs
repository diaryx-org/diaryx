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
}

impl<FS: AsyncFileSystem> SyncSession<FS> {
    /// Create a new sync session.
    pub fn new(config: SyncSessionConfig, sync_manager: Arc<RustSyncManager<FS>>) -> Self {
        Self {
            sync_manager,
            config,
            state: Mutex::new(SessionState::AwaitingConnect),
        }
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
            SessionState::Active => self.handle_control_message(text),
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
            }
            ControlMessage::SessionJoined { .. } => {
                log::info!("[SyncSession] Session joined during handshake");
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
                        if !result.changed_files.is_empty() {
                            log::debug!(
                                "[SyncSession] Workspace files changed: {:?}",
                                result.changed_files
                            );
                            actions.push(SessionAction::Emit(SyncEvent::FilesChanged {
                                files: result.changed_files,
                            }));
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
                match self
                    .sync_manager
                    .handle_body_message(&file_path, &payload, self.config.write_to_disk)
                    .await
                {
                    Ok(result) => {
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            actions.push(SessionAction::SendBinary(framed));
                        }
                        if result.content.is_some() && !result.is_echo {
                            log::debug!("[SyncSession] Body changed: {}", file_path);
                            actions.push(SessionAction::Emit(SyncEvent::BodyChanged {
                                file_path: file_path.clone(),
                            }));
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "[SyncSession] Error handling body message for {}: {}",
                            file_path,
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

    fn handle_control_message(&self, text: &str) -> Vec<SessionAction> {
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
                actions.push(SessionAction::Emit(SyncEvent::StatusChanged {
                    status: SyncStatus::Synced,
                }));
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

        // Send body SyncStep1 for all known files
        let file_paths = self.sync_manager.get_all_file_paths();
        let file_count = file_paths.len();

        if file_count > 0 {
            // Emit initial progress before sending any SyncStep1s
            actions.push(SessionAction::Emit(SyncEvent::Progress {
                completed: 0,
                total: file_count,
            }));
        }

        for (i, file_path) in file_paths.iter().enumerate() {
            let body_doc_id = format_body_doc_id(&self.config.workspace_id, file_path);
            let body_step1 = self.sync_manager.create_body_sync_step1(file_path);
            let body_framed = frame_message_v2(&body_doc_id, &body_step1);
            actions.push(SessionAction::SendBinary(body_framed));

            // Emit progress periodically for large workspaces
            if (i + 1) % 50 == 0 {
                actions.push(SessionAction::Emit(SyncEvent::Progress {
                    completed: i + 1,
                    total: file_count,
                }));
            }
        }

        if file_count > 0 {
            log::info!("[SyncSession] Sent body SyncStep1 for {} files", file_count);
        }

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
