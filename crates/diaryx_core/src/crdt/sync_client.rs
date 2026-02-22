//! Centralized sync client for native platforms (CLI, Tauri).
//!
//! `SyncClient` consolidates the duplicated WebSocket sync logic that was
//! previously copy-pasted between the CLI and Tauri frontends. It handles:
//!
//! - WebSocket connection via the `SyncTransport` trait
//! - Reconnection with exponential backoff
//! - Ping/keepalive
//! - Outgoing message channel (local CRDT changes → WebSocket)
//!
//! Protocol logic (handshake, message routing, framing, control messages) is
//! delegated to `SyncSession` in `sync_session.rs`, which is shared with WASM.
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
//!             SyncEvent::StatusChanged { status } => println!("Status: {:?}", status),
//!             SyncEvent::Progress { completed, total } => println!("{}/{}", completed, total),
//!             _ => {}
//!         }
//!     }
//! }
//!
//! use diaryx_core::crdt::TokioConnector;
//!
//! let client = SyncClient::new(config, sync_manager, Arc::new(MyHandler), TokioConnector);
//! client.run_persistent(running).await;
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::sync::{format_body_doc_id, format_workspace_doc_id};
use super::sync_manager::RustSyncManager;
use super::sync_session::{IncomingEvent, SessionAction, SyncSession};
use super::sync_types::{SyncEvent, SyncSessionConfig, SyncStatus};
use super::transport::{SyncTransport, TransportConnector, TransportError, WsMessage};
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
/// Wraps `SyncSession` and handles the WebSocket transport lifecycle
/// including connection, reconnection, and outgoing message channel.
/// Protocol logic is delegated to the shared `SyncSession`.
///
/// Generic over `C: TransportConnector` to allow different WebSocket
/// backends (e.g., `TokioConnector` for native, or a platform-specific
/// connector for iOS using `URLSessionWebSocketTask`).
pub struct SyncClient<FS: AsyncFileSystem, C: TransportConnector> {
    config: SyncClientConfig,
    sync_manager: Arc<RustSyncManager<FS>>,
    handler: Arc<dyn SyncEventHandler>,
    session: SyncSession<FS>,
    connector: C,
}

impl<FS: AsyncFileSystem + 'static, C: TransportConnector + 'static> SyncClient<FS, C> {
    /// Create a new sync client.
    pub fn new(
        config: SyncClientConfig,
        sync_manager: Arc<RustSyncManager<FS>>,
        handler: Arc<dyn SyncEventHandler>,
        connector: C,
    ) -> Self {
        let session_config = SyncSessionConfig {
            workspace_id: config.workspace_id.clone(),
            write_to_disk: true,
        };
        let session = SyncSession::new(session_config, Arc::clone(&sync_manager));
        Self {
            config,
            sync_manager,
            handler,
            session,
            connector,
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

    /// Execute a list of session actions via the transport.
    async fn execute_actions(
        &self,
        actions: Vec<SessionAction>,
        transport: &mut C::Transport,
    ) -> Result<(), TransportError> {
        for action in actions {
            match action {
                SessionAction::SendBinary(data) => {
                    transport.send_binary(data).await?;
                }
                SessionAction::SendText(text) => {
                    transport.send_text(text).await?;
                }
                SessionAction::Emit(event) => {
                    self.handler.on_event(event);
                }
                SessionAction::DownloadSnapshot { workspace_id } => {
                    // Native clients don't download snapshots via HTTP — send FilesReady
                    // immediately so the handshake continues (server will send CrdtState).
                    log::info!(
                        "[SyncClient] Snapshot download requested for {} — sending FilesReady (native)",
                        workspace_id
                    );
                    transport
                        .send_text(r#"{"type":"FilesReady"}"#.to_string())
                        .await?;
                }
            }
        }
        Ok(())
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
                self.handler.on_event(SyncEvent::StatusChanged {
                    status: SyncStatus::Reconnecting { attempt },
                });
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

            self.handler.on_event(SyncEvent::StatusChanged {
                status: SyncStatus::Connecting,
            });

            // Connect
            let mut transport = match self.connector.connect(&ws_url).await {
                Ok(t) => {
                    log::info!("[SyncClient] Connected to {}", ws_url);
                    attempt = 0; // Reset backoff on success
                    t
                }
                Err(e) => {
                    log::error!("[SyncClient] Connection failed: {}", e);
                    self.handler.on_event(SyncEvent::Error {
                        message: format!("Connection failed: {}", e),
                    });
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
                    self.handler.on_event(SyncEvent::Error {
                        message: e.to_string(),
                    });
                }
                attempt += 1;
                self.session.reset();
                self.sync_manager.reset();
                self.handler.on_event(SyncEvent::StatusChanged {
                    status: SyncStatus::Disconnected,
                });
            }
        }

        // Final cleanup
        self.sync_manager.clear_event_callback();
        self.handler.on_event(SyncEvent::StatusChanged {
            status: SyncStatus::Disconnected,
        });
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
        let mut transport = self.connector.connect(&ws_url).await?;

        // Use a one-shot session config (write_to_disk = false for push)
        let one_shot_session = SyncSession::new(
            SyncSessionConfig {
                workspace_id: self.config.workspace_id.clone(),
                write_to_disk: false,
            },
            Arc::clone(&self.sync_manager),
        );

        // Process Connected event
        let actions = one_shot_session.process(IncomingEvent::Connected).await;
        for action in actions {
            match action {
                SessionAction::SendBinary(data) => transport.send_binary(data).await?,
                SessionAction::SendText(text) => transport.send_text(text).await?,
                _ => {}
            }
        }

        // Handshake + collect SyncStep1s
        let hs_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);

        loop {
            tokio::select! {
                msg = transport.recv() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            let actions = one_shot_session.process(IncomingEvent::TextMessage(text)).await;

                            // Check if session transitioned to Active (Syncing status emitted)
                            let has_syncing = actions.iter().any(|a| matches!(
                                a, SessionAction::Emit(SyncEvent::StatusChanged { status: SyncStatus::Syncing })
                            ));

                            for action in actions {
                                match action {
                                    SessionAction::SendBinary(data) => transport.send_binary(data).await?,
                                    SessionAction::SendText(text) => transport.send_text(text).await?,
                                    SessionAction::DownloadSnapshot { workspace_id } => {
                                        // Native one-shot: send FilesReady to continue handshake
                                        log::info!(
                                            "[SyncClient] One-shot snapshot request for {} — sending FilesReady",
                                            workspace_id
                                        );
                                        transport
                                            .send_text(r#"{"type":"FilesReady"}"#.to_string())
                                            .await?;
                                    }
                                    _ => {}
                                }
                            }

                            if has_syncing {
                                break; // Handshake complete, session is Active
                            }
                            // Otherwise keep looping (e.g., FileManifest received, waiting for CrdtState)
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            // Server skipped handshake, process body step1s
                            let actions = one_shot_session.process(IncomingEvent::BinaryMessage(data)).await;
                            for action in actions {
                                match action {
                                    SessionAction::SendBinary(data) => transport.send_binary(data).await?,
                                    SessionAction::SendText(text) => transport.send_text(text).await?,
                                    _ => {}
                                }
                            }
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

        // Now in active state — exchange messages until sync is complete
        let file_paths = self.sync_manager.get_all_file_paths();
        let file_count = file_paths.len();
        let mut stats = SyncStats::default();
        let mut ws_handled = false;
        let mut body_files_handled: HashSet<String> = HashSet::new();

        let timeout_secs = (10 + file_count / 100).min(60) as u64;
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_secs);

        loop {
            let msg = tokio::select! {
                biased;
                msg = transport.recv() => msg,
                _ = tokio::time::sleep_until(deadline) => break,
            };
            match msg {
                Some(Ok(WsMessage::Binary(data))) => {
                    // Track stats before processing
                    if let Some((doc_id, _payload)) = unframe_message_v2(&data) {
                        match parse_doc_id(&doc_id) {
                            Some(DocIdKind::Workspace(_)) => {
                                ws_handled = true;
                            }
                            Some(DocIdKind::Body { file_path, .. }) => {
                                body_files_handled.insert(file_path);
                            }
                            None => {}
                        }
                    }

                    let actions = one_shot_session
                        .process(IncomingEvent::BinaryMessage(data))
                        .await;
                    for action in actions {
                        match action {
                            SessionAction::SendBinary(data) => {
                                transport.send_binary(data).await?;
                                stats.pushed += 1;
                            }
                            SessionAction::Emit(SyncEvent::FilesChanged { files })
                                if !files.is_empty() =>
                            {
                                stats.pulled += files.len();
                            }
                            SessionAction::Emit(SyncEvent::BodyChanged { .. }) => {
                                stats.pulled += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Some(Ok(WsMessage::Text(text))) => {
                    let actions = one_shot_session
                        .process(IncomingEvent::TextMessage(text))
                        .await;
                    for action in actions {
                        match action {
                            SessionAction::SendBinary(data) => transport.send_binary(data).await?,
                            SessionAction::SendText(text) => transport.send_text(text).await?,
                            _ => {}
                        }
                    }
                }
                Some(Ok(WsMessage::Close)) | None => break,
                Some(Err(e)) => return Err(e),
                _ => continue,
            }

            if ws_handled && body_files_handled.len() >= file_count {
                break;
            }
        }

        let _ = transport.close().await;
        Ok(stats)
    }

    /// Run a single sync session (after connection is established).
    ///
    /// Feeds transport messages into `SyncSession::process()` and executes actions.
    async fn run_session(
        &self,
        transport: &mut C::Transport,
        outgoing_rx: &mut tokio::sync::mpsc::UnboundedReceiver<(String, Vec<u8>)>,
        running: &Arc<AtomicBool>,
    ) -> Result<(), TransportError> {
        // Process Connected event
        let actions = self.session.process(IncomingEvent::Connected).await;
        self.execute_actions(actions, transport).await?;

        // Handshake loop: feed messages until session transitions to Active
        let handshake_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);

        loop {
            tokio::select! {
                msg = transport.recv() => {
                    match msg {
                        Some(Ok(WsMessage::Text(text))) => {
                            let actions = self.session.process(IncomingEvent::TextMessage(text)).await;

                            // Check if we transitioned to Active (CrdtState triggers body SyncStep1s)
                            let has_syncing = actions.iter().any(|a| matches!(
                                a, SessionAction::Emit(SyncEvent::StatusChanged { status: SyncStatus::Syncing })
                            ));

                            self.execute_actions(actions, transport).await?;

                            if has_syncing {
                                break; // Handshake complete
                            }
                        }
                        Some(Ok(WsMessage::Binary(data))) => {
                            // Server skipped handshake
                            let actions = self.session.process(IncomingEvent::BinaryMessage(data)).await;
                            self.execute_actions(actions, transport).await?;
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

        // Main message loop
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
                            let actions = self.session.process(IncomingEvent::BinaryMessage(data)).await;
                            self.execute_actions(actions, transport).await?;
                        }
                        Some(Ok(WsMessage::Text(text))) => {
                            let actions = self.session.process(IncomingEvent::TextMessage(text)).await;
                            self.execute_actions(actions, transport).await?;
                        }
                        Some(Ok(WsMessage::Close)) => {
                            log::info!("[SyncClient] Connection closed by server");
                            break;
                        }
                        Some(Ok(WsMessage::Pong(_))) => {} // keepalive
                        Some(Err(e)) => {
                            log::error!("[SyncClient] WebSocket error: {}", e);
                            self.handler.on_event(SyncEvent::Error { message: e.to_string() });
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                outgoing = outgoing_rx.recv() => {
                    if let Some((doc_id, message)) = outgoing {
                        let actions = self.session.process(IncomingEvent::LocalUpdate { doc_id, data: message }).await;
                        self.execute_actions(actions, transport).await?;
                    }
                }
                _ = ping_interval.tick() => {
                    transport.send_ping().await?;
                }
            }
        }

        Ok(())
    }
}
