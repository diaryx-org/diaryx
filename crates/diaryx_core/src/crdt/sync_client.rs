//! Centralized sync client for native platforms (CLI, Tauri).
//!
//! `SyncClient` consolidates the duplicated WebSocket sync logic that was
//! previously copy-pasted between the CLI and Tauri frontends. It handles:
//!
//! - WebSocket connection via the `SyncTransport` trait
//! - Files-Ready handshake protocol
//! - Body SyncStep1 loop for all tracked files
//! - Binary message routing (workspace vs body)
//! - JSON control message dispatch
//! - Outgoing message channel (local CRDT changes → WebSocket)
//! - Reconnection with exponential backoff
//! - Ping/keepalive
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_core::crdt::{SyncClient, SyncClientConfig, SyncEvent, SyncEventHandler};
//!
//! struct MyHandler;
//! impl SyncEventHandler for MyHandler {
//!     fn on_event(&self, event: SyncEvent) {
//!         match event {
//!             SyncEvent::StatusChanged(status) => println!("Status: {:?}", status),
//!             SyncEvent::Progress { completed, total } => println!("{}/{}", completed, total),
//!             _ => {}
//!         }
//!     }
//! }
//!
//! let client = SyncClient::new(config, sync_manager, Arc::new(MyHandler));
//! client.run_persistent(running).await;
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine;

use super::control_message::ControlMessage;
use super::sync::{format_body_doc_id, format_workspace_doc_id, frame_message_v2};
use super::sync_manager::RustSyncManager;
use super::tokio_transport::TokioTransport;
use super::transport::{SyncTransport, TransportError, WsMessage};
use crate::fs::{AsyncFileSystem, FileSystemEvent};

/// Configuration for the sync client.
#[derive(Debug, Clone)]
pub struct SyncClientConfig {
    /// Base server URL (e.g., "https://sync.diaryx.org").
    pub server_url: String,
    /// Workspace ID to sync.
    pub workspace_id: String,
    /// Authentication token (session token or share token).
    pub auth_token: Option<String>,
    /// Reconnection configuration.
    pub reconnect: ReconnectConfig,
}

/// Reconnection configuration.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Whether to automatically reconnect on disconnect.
    pub enabled: bool,
    /// Maximum number of reconnection attempts (0 = infinite).
    pub max_attempts: u32,
    /// Base delay in seconds for exponential backoff.
    pub base_delay_secs: u64,
    /// Maximum delay in seconds for exponential backoff.
    pub max_delay_secs: u64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 10,
            base_delay_secs: 2,
            max_delay_secs: 32,
        }
    }
}

/// Events emitted by the sync client to the frontend.
#[derive(Debug)]
pub enum SyncEvent {
    /// Sync status changed.
    StatusChanged(SyncStatus),
    /// Sync progress update.
    Progress {
        /// Number of files completed.
        completed: usize,
        /// Total number of files.
        total: usize,
    },
    /// Workspace files changed (metadata sync).
    FilesChanged(Vec<String>),
    /// A body document changed.
    BodyChanged {
        /// Path of the changed file.
        file_path: String,
    },
    /// An error occurred.
    Error(String),
}

/// Current sync status.
#[derive(Debug, Clone)]
pub enum SyncStatus {
    /// Connecting to the server.
    Connecting,
    /// Connected to the server.
    Connected,
    /// Performing initial sync.
    Syncing,
    /// Initial sync complete, watching for changes.
    Synced,
    /// Reconnecting after disconnect.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
    },
    /// Disconnected from the server.
    Disconnected,
}

/// Trait for receiving sync events.
///
/// Implementors translate `SyncEvent`s into frontend-specific actions
/// (e.g., CLI prints, Tauri event emissions).
pub trait SyncEventHandler: Send + Sync {
    /// Called when a sync event occurs.
    fn on_event(&self, event: SyncEvent);
}

/// Statistics from a one-shot sync operation.
#[derive(Debug, Default)]
pub struct SyncStats {
    /// Number of items pushed to the server.
    pub pushed: usize,
    /// Number of items pulled from the server.
    pub pulled: usize,
}

/// Centralized sync client for native platforms.
///
/// Wraps `RustSyncManager` and handles the full WebSocket sync lifecycle
/// including connection, handshake, message routing, and reconnection.
pub struct SyncClient<FS: AsyncFileSystem> {
    config: SyncClientConfig,
    sync_manager: Arc<RustSyncManager<FS>>,
    handler: Arc<dyn SyncEventHandler>,
}

impl<FS: AsyncFileSystem + 'static> SyncClient<FS> {
    /// Create a new sync client.
    pub fn new(
        config: SyncClientConfig,
        sync_manager: Arc<RustSyncManager<FS>>,
        handler: Arc<dyn SyncEventHandler>,
    ) -> Self {
        Self {
            config,
            sync_manager,
            handler,
        }
    }

    /// Build the WebSocket URL from the config.
    fn build_ws_url(&self) -> String {
        let ws_server = self
            .config
            .server_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        if let Some(ref token) = self.config.auth_token {
            format!("{}/sync2?token={}", ws_server, token)
        } else {
            format!("{}/sync2", ws_server)
        }
    }

    /// Run persistent sync with reconnection.
    ///
    /// This is the main entry point for continuous sync (replaces both
    /// CLI's `run_sync_loop_v2` and Tauri's `start_websocket_sync`).
    ///
    /// The loop runs until `running` is set to `false` or max reconnection
    /// attempts are exhausted.
    pub async fn run_persistent(&self, running: Arc<AtomicBool>) {
        let ws_url = self.build_ws_url();
        let mut attempt = 0u32;
        let rc = &self.config.reconnect;

        // Set up outgoing message channel for local CRDT changes.
        let (outgoing_tx, mut outgoing_rx) =
            tokio::sync::mpsc::unbounded_channel::<(String, Vec<u8>)>();

        // Wire up the sync_manager event callback.
        {
            let outgoing_tx = outgoing_tx.clone();
            let ws_id = self.config.workspace_id.clone();
            self.sync_manager.set_event_callback(Arc::new(move |event| {
                if let FileSystemEvent::SendSyncMessage {
                    doc_name,
                    message,
                    is_body,
                    ..
                } = event
                {
                    let doc_id = if *is_body {
                        format_body_doc_id(&ws_id, doc_name)
                    } else {
                        format_workspace_doc_id(&ws_id)
                    };
                    let _ = outgoing_tx.send((doc_id, message.clone()));
                }
            }));
        }

        while running.load(Ordering::SeqCst) {
            if rc.max_attempts > 0 && attempt >= rc.max_attempts {
                log::info!("[SyncClient] Max reconnection attempts reached");
                break;
            }

            // Backoff delay on reconnection
            if attempt > 0 {
                let delay = std::cmp::min(rc.base_delay_secs.pow(attempt), rc.max_delay_secs);
                self.handler
                    .on_event(SyncEvent::StatusChanged(SyncStatus::Reconnecting {
                        attempt,
                    }));
                log::info!(
                    "[SyncClient] Reconnecting in {}s (attempt {}/{})",
                    delay,
                    attempt,
                    if rc.max_attempts == 0 {
                        "∞".to_string()
                    } else {
                        rc.max_attempts.to_string()
                    }
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }

            self.handler
                .on_event(SyncEvent::StatusChanged(SyncStatus::Connecting));

            // Connect
            let mut transport = match TokioTransport::connect(&ws_url).await {
                Ok(t) => {
                    log::info!("[SyncClient] Connected to {}", ws_url);
                    self.handler
                        .on_event(SyncEvent::StatusChanged(SyncStatus::Connected));
                    attempt = 0; // Reset backoff on success
                    t
                }
                Err(e) => {
                    log::error!("[SyncClient] Connection failed: {}", e);
                    self.handler
                        .on_event(SyncEvent::Error(format!("Connection failed: {}", e)));
                    attempt += 1;
                    continue;
                }
            };

            // Run the sync session (handshake + message loop)
            let result = self
                .run_session(&mut transport, &mut outgoing_rx, &running)
                .await;

            // Connection dropped
            let _ = transport.close().await;
            if running.load(Ordering::SeqCst) {
                if let Err(e) = result {
                    log::error!("[SyncClient] Session error: {}", e);
                    self.handler.on_event(SyncEvent::Error(e.to_string()));
                }
                attempt += 1;
                self.sync_manager.reset();
                self.handler
                    .on_event(SyncEvent::StatusChanged(SyncStatus::Disconnected));
            }
        }

        // Final cleanup
        self.sync_manager.clear_event_callback();
        self.handler
            .on_event(SyncEvent::StatusChanged(SyncStatus::Disconnected));
        log::info!("[SyncClient] Sync loop exited");
    }

    /// Run a one-shot sync (push + pull), then disconnect.
    ///
    /// Replaces CLI's `do_one_shot_sync_v2`. Connects, performs the handshake,
    /// exchanges SyncStep1/SyncStep2 for workspace and all bodies, then disconnects.
    pub async fn run_one_shot(&self) -> Result<SyncStats, TransportError> {
        use super::sync::{DocIdKind, parse_doc_id, unframe_message_v2};
        use std::collections::HashSet;

        let ws_url = self.build_ws_url();
        let mut transport = TokioTransport::connect(&ws_url).await?;

        // Send workspace SyncStep1
        let ws_doc_id = format_workspace_doc_id(&self.config.workspace_id);
        let ws_step1 = self.sync_manager.create_workspace_sync_step1();
        let ws_framed = frame_message_v2(&ws_doc_id, &ws_step1);
        transport.send_binary(ws_framed).await?;

        // Handshake
        let mut stashed_binary: Option<Vec<u8>> = None;
        let hs_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);

        loop {
            tokio::select! {
                msg = transport.recv() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(ctrl) = serde_json::from_str::<ControlMessage>(&text) {
                                match ctrl {
                                    ControlMessage::FileManifest { .. } => {
                                        transport.send_text(r#"{"type":"FilesReady"}"#.to_string()).await?;
                                    }
                                    ControlMessage::CrdtState { state } => {
                                        if let Ok(state_bytes) = base64::engine::general_purpose::STANDARD.decode(&state) {
                                            let _ = self.sync_manager.handle_crdt_state(&state_bytes).await;
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            stashed_binary = Some(data);
                            break;
                        }
                        Some(Ok(WsMessage::Close)) | None => {
                            let _ = transport.close().await;
                            return Ok(SyncStats::default());
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(hs_deadline) => {
                    break;
                }
            }
        }

        // Get files to sync
        let file_paths = self.sync_manager.get_all_file_paths();
        let file_count = file_paths.len();

        // Send body SyncStep1 for all files
        for file_path in &file_paths {
            let body_doc_id = format_body_doc_id(&self.config.workspace_id, file_path);
            let body_step1 = self.sync_manager.create_body_sync_step1(file_path);
            let body_framed = frame_message_v2(&body_doc_id, &body_step1);
            transport.send_binary(body_framed).await?;
        }

        let mut stats = SyncStats::default();
        let mut ws_handled = false;
        let mut body_files_handled: HashSet<String> = HashSet::new();

        // Timeout based on file count
        let timeout_secs = (10 + file_count / 100).min(60) as u64;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

        // Process stashed binary + incoming
        let mut pending: Vec<Vec<u8>> = Vec::new();
        if let Some(data) = stashed_binary.take() {
            pending.push(data);
        }

        loop {
            let data = if let Some(data) = pending.pop() {
                data
            } else {
                let msg = tokio::select! {
                    biased;
                    msg = transport.recv() => msg,
                    _ = tokio::time::sleep_until(deadline) => break,
                };
                match msg {
                    Some(Ok(WsMessage::Binary(data))) => data,
                    Some(Ok(WsMessage::Text(_))) => continue,
                    Some(Ok(WsMessage::Close)) | None => break,
                    Some(Err(e)) => return Err(e),
                    _ => continue,
                }
            };

            if let Some((doc_id, payload)) = unframe_message_v2(&data) {
                match parse_doc_id(&doc_id) {
                    Some(DocIdKind::Workspace(_)) => {
                        // Delegate to sync_manager which decodes and processes internally
                        if let Ok(result) = self
                            .sync_manager
                            .handle_workspace_message(&payload, false)
                            .await
                        {
                            if let Some(response) = result.response {
                                let framed = frame_message_v2(&doc_id, &response);
                                transport.send_binary(framed).await?;
                                stats.pushed += 1;
                            }
                            if !result.changed_files.is_empty() {
                                stats.pulled += result.changed_files.len();
                            }
                        }
                        ws_handled = true;
                    }
                    Some(DocIdKind::Body { file_path, .. }) => {
                        // Delegate to sync_manager which decodes and processes internally
                        if let Ok(result) = self
                            .sync_manager
                            .handle_body_message(&file_path, &payload, false)
                            .await
                        {
                            if let Some(response) = result.response {
                                let framed = frame_message_v2(&doc_id, &response);
                                transport.send_binary(framed).await?;
                            }
                            if result.content.is_some() {
                                stats.pulled += 1;
                            }
                        }
                        body_files_handled.insert(file_path);
                    }
                    None => {}
                }
            }

            // Check if sync is complete (received at least one workspace message
            // and handled all body files)
            if ws_handled && body_files_handled.len() >= file_count {
                break;
            }
        }

        let _ = transport.close().await;
        Ok(stats)
    }

    /// Run a single sync session (after connection is established).
    ///
    /// Performs handshake, sends body SyncStep1s, then enters the main message loop.
    /// Returns `Ok(())` on graceful close, `Err` on transport errors.
    async fn run_session(
        &self,
        transport: &mut TokioTransport,
        outgoing_rx: &mut tokio::sync::mpsc::UnboundedReceiver<(String, Vec<u8>)>,
        running: &Arc<AtomicBool>,
    ) -> Result<(), TransportError> {
        let workspace_id = &self.config.workspace_id;

        // --- Send workspace SyncStep1 ---
        let ws_doc_id = format_workspace_doc_id(workspace_id);
        let ws_step1 = self.sync_manager.create_workspace_sync_step1();
        let ws_framed = frame_message_v2(&ws_doc_id, &ws_step1);
        transport.send_binary(ws_framed).await?;

        // --- Files-Ready handshake ---
        let mut stashed_binary: Option<Vec<u8>> = None;
        let handshake_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);

        loop {
            tokio::select! {
                msg = transport.recv() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(ctrl) = serde_json::from_str::<ControlMessage>(&text) {
                                match ctrl {
                                    ControlMessage::FileManifest { .. } => {
                                        log::info!("[SyncClient] Received FileManifest, sending FilesReady");
                                        transport.send_text(r#"{"type":"FilesReady"}"#.to_string()).await?;
                                    }
                                    ControlMessage::CrdtState { state } => {
                                        match base64::engine::general_purpose::STANDARD.decode(&state) {
                                            Ok(state_bytes) => {
                                                match self.sync_manager.handle_crdt_state(&state_bytes).await {
                                                    Ok(count) => {
                                                        log::info!("[SyncClient] Applied CRDT state ({} files)", count);
                                                        self.handler.on_event(SyncEvent::FilesChanged(vec![]));
                                                    }
                                                    Err(e) => {
                                                        log::error!("[SyncClient] Failed to apply CRDT state: {}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                log::error!("[SyncClient] Failed to decode CRDT state: {}", e);
                                            }
                                        }
                                        break; // Handshake complete
                                    }
                                    ControlMessage::SessionJoined { .. } => {
                                        log::info!("[SyncClient] Session joined");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            // Server returned Continue (no handshake needed).
                            stashed_binary = Some(data);
                            break;
                        }
                        Some(Ok(WsMessage::Close)) | None => {
                            log::warn!("[SyncClient] Connection closed during handshake");
                            return Err(TransportError::Closed);
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(handshake_deadline) => {
                    log::debug!("[SyncClient] No handshake required, proceeding");
                    break;
                }
            }
        }

        self.handler
            .on_event(SyncEvent::StatusChanged(SyncStatus::Syncing));

        // --- Send body SyncStep1 for all known files ---
        let file_paths = self.sync_manager.get_all_file_paths();
        let file_count = file_paths.len();
        let mut sent = 0;
        for file_path in &file_paths {
            if !running.load(Ordering::SeqCst) {
                break;
            }
            let body_doc_id = format_body_doc_id(workspace_id, file_path);
            let body_step1 = self.sync_manager.create_body_sync_step1(file_path);
            let body_framed = frame_message_v2(&body_doc_id, &body_step1);
            transport.send_binary(body_framed).await?;
            sent += 1;

            // Yield periodically for large workspaces
            if sent % 50 == 0 {
                self.handler.on_event(SyncEvent::Progress {
                    completed: sent,
                    total: file_count,
                });
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        if file_count > 0 {
            log::info!("[SyncClient] Sent body SyncStep1 for {} files", file_count);
            self.handler.on_event(SyncEvent::Progress {
                completed: 0,
                total: file_count,
            });
        }

        // --- Process stashed binary from handshake ---
        if let Some(data) = stashed_binary.take() {
            if let Some(response) = self.handle_binary_message(&data).await {
                transport.send_binary(response).await?;
            }
        }

        // --- Main message loop ---
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        ping_interval.tick().await; // Consume first immediate tick

        loop {
            if !running.load(Ordering::SeqCst) {
                break;
            }

            tokio::select! {
                msg = transport.recv() => {
                    match msg {
                        Some(Ok(WsMessage::Binary(data))) => {
                            if let Some(response) = self.handle_binary_message(&data).await {
                                transport.send_binary(response).await?;
                            }
                        }
                        Some(Ok(WsMessage::Text(text))) => {
                            self.handle_control_message(&text);
                        }
                        Some(Ok(WsMessage::Close)) => {
                            log::info!("[SyncClient] Connection closed by server");
                            break;
                        }
                        Some(Ok(WsMessage::Pong(_))) => {} // keepalive
                        Some(Err(e)) => {
                            log::error!("[SyncClient] WebSocket error: {}", e);
                            self.handler.on_event(SyncEvent::Error(e.to_string()));
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                outgoing = outgoing_rx.recv() => {
                    if let Some((doc_id, message)) = outgoing {
                        let framed = frame_message_v2(&doc_id, &message);
                        transport.send_binary(framed).await?;
                    }
                }
                _ = ping_interval.tick() => {
                    transport.send_ping().await?;
                }
            }
        }

        Ok(())
    }

    /// Handle a binary V2 message: unframe, route to sync_manager, return optional response.
    async fn handle_binary_message(&self, data: &[u8]) -> Option<Vec<u8>> {
        use super::sync::{DocIdKind, parse_doc_id, unframe_message_v2};

        let (doc_id, payload) = unframe_message_v2(data)?;
        match parse_doc_id(&doc_id) {
            Some(DocIdKind::Workspace(_)) => {
                match self
                    .sync_manager
                    .handle_workspace_message(&payload, true)
                    .await
                {
                    Ok(result) => {
                        if !result.changed_files.is_empty() {
                            log::debug!(
                                "[SyncClient] Workspace files changed: {:?}",
                                result.changed_files
                            );
                            self.handler
                                .on_event(SyncEvent::FilesChanged(result.changed_files));
                        }
                        result.response.map(|resp| frame_message_v2(&doc_id, &resp))
                    }
                    Err(e) => {
                        log::error!("[SyncClient] Error handling workspace message: {}", e);
                        self.handler.on_event(SyncEvent::Error(e.to_string()));
                        None
                    }
                }
            }
            Some(DocIdKind::Body { file_path, .. }) => {
                match self
                    .sync_manager
                    .handle_body_message(&file_path, &payload, true)
                    .await
                {
                    Ok(result) => {
                        if result.content.is_some() && !result.is_echo {
                            log::debug!("[SyncClient] Body changed: {}", file_path);
                            self.handler.on_event(SyncEvent::BodyChanged {
                                file_path: file_path.clone(),
                            });
                        }
                        result.response.map(|resp| frame_message_v2(&doc_id, &resp))
                    }
                    Err(e) => {
                        log::error!(
                            "[SyncClient] Error handling body message for {}: {}",
                            file_path,
                            e
                        );
                        None
                    }
                }
            }
            None => {
                log::debug!("[SyncClient] Unknown doc_id: {}", doc_id);
                None
            }
        }
    }

    /// Handle a JSON control message.
    fn handle_control_message(&self, text: &str) {
        if let Ok(ctrl) = serde_json::from_str::<ControlMessage>(text) {
            match ctrl {
                ControlMessage::SyncProgress { completed, total } => {
                    log::debug!("[SyncClient] Progress: {}/{}", completed, total);
                    self.handler
                        .on_event(SyncEvent::Progress { completed, total });
                }
                ControlMessage::SyncComplete { files_synced } => {
                    log::info!("[SyncClient] Sync complete ({} files)", files_synced);
                    self.handler
                        .on_event(SyncEvent::StatusChanged(SyncStatus::Synced));
                }
                ControlMessage::PeerJoined { peer_count } => {
                    log::info!("[SyncClient] Peer joined ({} connected)", peer_count);
                }
                ControlMessage::PeerLeft { peer_count } => {
                    log::info!("[SyncClient] Peer left ({} connected)", peer_count);
                }
                ControlMessage::FocusListChanged { files } => {
                    if !files.is_empty() {
                        log::debug!("[SyncClient] Focus list changed: {} files", files.len());
                    }
                }
                _ => {}
            }
        }
    }
}
