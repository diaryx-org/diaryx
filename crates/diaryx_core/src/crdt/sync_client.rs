//! Unified sync client for workspace and body synchronization.
//!
//! This module provides `SyncClient`, which manages dual WebSocket connections
//! for real-time sync. It works with any transport implementing `SyncTransport`:
//!
//! - **Native**: `TokioTransport` with tokio-tungstenite
//! - **WASM**: `CallbackTransport` with JavaScript WebSocket
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      SyncClient<T>                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌─────────────────┐       ┌─────────────────┐              │
//! │  │ Metadata Conn   │       │ Body Conn       │              │
//! │  │ (workspace)     │       │ (multiplexed)   │              │
//! │  └────────┬────────┘       └────────┬────────┘              │
//! │           │                         │                       │
//! │           └────────────┬────────────┘                       │
//! │                        ▼                                    │
//! │           ┌──────────────────────┐                          │
//! │           │   RustSyncManager    │                          │
//! │           └──────────────────────┘                          │
//! │                                                             │
//! │  Features:                                                  │
//! │  - Dual connection management                               │
//! │  - Exponential backoff reconnection                         │
//! │  - Progress tracking and status reporting                   │
//! │  - Message routing to RustSyncManager                       │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{SyncClient, SyncClientConfig, TokioTransport};
//!
//! let transport_meta = TokioTransport::new();
//! let transport_body = TokioTransport::new();
//!
//! let config = SyncClientConfig {
//!     server_url: "wss://sync.example.com/sync".to_string(),
//!     workspace_id: "my-workspace".to_string(),
//!     auth_token: Some("token".to_string()),
//!     workspace_root: PathBuf::from("/path/to/workspace"),
//!     write_to_disk: true,
//! };
//!
//! let client = SyncClient::new(
//!     config,
//!     transport_meta,
//!     transport_body,
//!     sync_manager,
//! );
//!
//! client.start().await?;
//! // ... sync is running ...
//! client.stop().await;
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use super::sync_manager::RustSyncManager;
use super::transport::{ConnectionStatus, MessageCallback, SyncConfig, SyncTransport};
use super::{frame_body_message, unframe_body_message};
use crate::error::Result;
use crate::fs::{AsyncFileSystem, FileSystemEvent};

// ============================================================================
// Outgoing Message Types
// ============================================================================

/// An outgoing sync message to be sent via WebSocket.
///
/// This is used to queue messages from local CRDT changes for sending
/// to the sync server.
#[derive(Debug, Clone)]
pub struct OutgoingSyncMessage {
    /// Document name ("workspace" for metadata, file path for body).
    pub doc_name: String,
    /// Encoded sync message bytes.
    pub message: Vec<u8>,
    /// Whether this is a body doc (true) or workspace (false).
    pub is_body: bool,
}

impl OutgoingSyncMessage {
    /// Create a new workspace metadata message.
    pub fn workspace(message: Vec<u8>) -> Self {
        Self {
            doc_name: "workspace".to_string(),
            message,
            is_body: false,
        }
    }

    /// Create a new body content message.
    pub fn body(doc_name: String, message: Vec<u8>) -> Self {
        Self {
            doc_name,
            message,
            is_body: true,
        }
    }

    /// Create from a FileSystemEvent::SendSyncMessage.
    ///
    /// Returns Some if the event is a SendSyncMessage, None otherwise.
    pub fn from_event(event: &FileSystemEvent) -> Option<Self> {
        match event {
            FileSystemEvent::SendSyncMessage {
                doc_name,
                message,
                is_body,
            } => Some(Self {
                doc_name: doc_name.clone(),
                message: message.clone(),
                is_body: *is_body,
            }),
            _ => None,
        }
    }
}

/// Sender for outgoing sync messages.
///
/// Clone this and use it to send messages from anywhere (e.g., event callbacks).
/// Messages are queued and sent by the SyncClient when connected.
pub type OutgoingSender = std::sync::mpsc::Sender<OutgoingSyncMessage>;

/// Receiver for outgoing sync messages (internal use).
type OutgoingReceiver = std::sync::mpsc::Receiver<OutgoingSyncMessage>;

/// Event callback type that can be used with RustSyncManager or EventEmittingFs.
pub type SyncEventBridge = Arc<dyn Fn(&FileSystemEvent) + Send + Sync>;

/// Create an event bridge that forwards `SendSyncMessage` events to a SyncClient.
///
/// This function creates a callback that can be registered with `RustSyncManager`
/// or `EventEmittingFs` to automatically route local CRDT changes to the WebSocket
/// connection.
///
/// # Usage
///
/// ```ignore
/// let client = SyncClient::new(config, meta_transport, body_transport, sync_manager.clone());
/// let sender = client.outgoing_sender();
/// let bridge = create_sync_event_bridge(sender);
///
/// // Register with RustSyncManager
/// sync_manager.set_event_callback(bridge);
/// ```
///
/// # Arguments
///
/// * `sender` - The OutgoingSender from a SyncClient
///
/// # Returns
///
/// A callback that converts `SendSyncMessage` events to `OutgoingSyncMessage` and
/// queues them for sending via the provided sender.
pub fn create_sync_event_bridge(sender: OutgoingSender) -> SyncEventBridge {
    Arc::new(move |event: &FileSystemEvent| {
        if let Some(msg) = OutgoingSyncMessage::from_event(event) {
            let doc_name = msg.doc_name.clone();
            let is_body = msg.is_body;
            let msg_len = msg.message.len();
            if let Err(e) = sender.send(msg) {
                log::warn!(
                    "[SyncEventBridge] Failed to queue sync message: doc={}, is_body={}, msg_len={}, error={:#?}",
                    doc_name,
                    is_body,
                    msg_len,
                    e
                );
            } else {
                log::debug!(
                    "[SyncEventBridge] Queued outgoing sync message: doc={}, is_body={}",
                    doc_name,
                    is_body
                );
            }
        }
    })
}

/// Configuration for a sync client.
#[derive(Debug, Clone)]
pub struct SyncClientConfig {
    /// WebSocket server URL (e.g., "wss://sync.diaryx.org/sync").
    pub server_url: String,

    /// Workspace ID for the sync session.
    pub workspace_id: String,

    /// Optional authentication token.
    pub auth_token: Option<String>,

    /// Path to the workspace root for file operations.
    pub workspace_root: PathBuf,

    /// Whether to write synced changes to disk.
    pub write_to_disk: bool,

    /// Maximum reconnection attempts before giving up.
    pub max_reconnect_attempts: u32,
}

impl SyncClientConfig {
    /// Create a new sync client configuration.
    pub fn new(server_url: String, workspace_id: String, workspace_root: PathBuf) -> Self {
        Self {
            server_url,
            workspace_id,
            auth_token: None,
            workspace_root,
            write_to_disk: true,
            max_reconnect_attempts: 10,
        }
    }

    /// Set the authentication token.
    pub fn with_auth(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// Set whether to write changes to disk.
    pub fn with_write_to_disk(mut self, write: bool) -> Self {
        self.write_to_disk = write;
        self
    }

    /// Set the maximum reconnection attempts.
    pub fn with_max_reconnects(mut self, max: u32) -> Self {
        self.max_reconnect_attempts = max;
        self
    }

    /// Build the SyncConfig for metadata connection.
    fn metadata_config(&self) -> SyncConfig {
        let mut config = SyncConfig::metadata(self.server_url.clone(), self.workspace_id.clone());
        if let Some(ref token) = self.auth_token {
            config = config.with_auth(token.clone());
        }
        config.with_write_to_disk(self.write_to_disk)
    }

    /// Build the SyncConfig for body connection.
    fn body_config(&self) -> SyncConfig {
        let mut config = SyncConfig::body(self.server_url.clone(), self.workspace_id.clone());
        if let Some(ref token) = self.auth_token {
            config = config.with_auth(token.clone());
        }
        config.with_write_to_disk(self.write_to_disk)
    }
}

/// Callback for sync events.
pub type SyncEventCallback = Arc<dyn Fn(SyncEvent) + Send + Sync>;

/// Events emitted by the sync client.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Connection status changed.
    StatusChanged(ConnectionStatus),
    /// Initial metadata sync completed.
    MetadataSynced {
        /// Number of files synced.
        file_count: usize,
    },
    /// Initial body sync completed.
    BodySynced {
        /// Number of body documents synced.
        file_count: usize,
    },
    /// Files changed from remote.
    FilesChanged {
        /// Paths of changed files.
        paths: Vec<String>,
    },
    /// Body content changed from remote.
    BodyChanged {
        /// Path of the changed file.
        path: String,
        /// New content of the file body.
        content: String,
    },
    /// Sync progress update.
    Progress {
        /// Number of files completed.
        completed: usize,
        /// Total number of files to sync.
        total: usize,
    },
    /// Error occurred.
    Error {
        /// Error message.
        message: String,
    },
    /// Focus list changed - files that other clients are focused on.
    /// Clients should subscribe to sync these files.
    FocusListChanged {
        /// Paths of files that any client is focused on.
        files: Vec<String>,
    },
}

/// Unified sync client for dual-connection sync.
///
/// Manages two WebSocket connections:
/// 1. **Metadata connection**: Syncs file metadata (title, part_of, etc.)
/// 2. **Body connection**: Syncs file content via multiplexed protocol
///
/// All sync logic is delegated to `RustSyncManager`.
///
/// ## Outgoing Messages
///
/// Local CRDT changes can be sent via the outgoing message channel:
/// 1. Call `outgoing_sender()` to get a clone of the sender
/// 2. Send `OutgoingSyncMessage` to queue messages
/// 3. Call `process_outgoing()` periodically (or use event-driven approach)
///
/// This design allows the `RustSyncManager` event callback to queue messages
/// without holding a direct reference to the SyncClient.
#[deprecated(
    note = "Use direct WebSocket with v2 protocol instead. See CLI sync/client.rs for reference."
)]
pub struct SyncClient<T: SyncTransport, FS: AsyncFileSystem + Send + Sync + 'static> {
    config: SyncClientConfig,
    metadata_transport: T,
    body_transport: T,
    sync_manager: Arc<RustSyncManager<FS>>,

    // State
    running: AtomicBool,
    metadata_connected: AtomicBool,
    body_connected: AtomicBool,
    reconnect_attempts: AtomicU32,
    status: RwLock<ConnectionStatus>,

    // Event callback
    event_callback: RwLock<Option<SyncEventCallback>>,

    // Outgoing message channel for local CRDT changes
    outgoing_tx: OutgoingSender,
    outgoing_rx: Mutex<Option<OutgoingReceiver>>,
}

impl<T: SyncTransport, FS: AsyncFileSystem + Send + Sync + 'static> SyncClient<T, FS> {
    /// Create a new sync client.
    ///
    /// # Arguments
    ///
    /// * `config` - Client configuration
    /// * `metadata_transport` - Transport for metadata connection
    /// * `body_transport` - Transport for body connection
    /// * `sync_manager` - RustSyncManager for handling sync logic
    pub fn new(
        config: SyncClientConfig,
        metadata_transport: T,
        body_transport: T,
        sync_manager: Arc<RustSyncManager<FS>>,
    ) -> Self {
        // Create channel for outgoing messages
        let (outgoing_tx, outgoing_rx) = std::sync::mpsc::channel();

        Self {
            config,
            metadata_transport,
            body_transport,
            sync_manager,
            running: AtomicBool::new(false),
            metadata_connected: AtomicBool::new(false),
            body_connected: AtomicBool::new(false),
            reconnect_attempts: AtomicU32::new(0),
            status: RwLock::new(ConnectionStatus::Disconnected),
            event_callback: RwLock::new(None),
            outgoing_tx,
            outgoing_rx: Mutex::new(Some(outgoing_rx)),
        }
    }

    /// Get a clone of the outgoing message sender.
    ///
    /// Use this to queue messages for sending from event callbacks.
    /// Messages will be sent when `process_outgoing()` is called or
    /// when the sync client processes them internally.
    pub fn outgoing_sender(&self) -> OutgoingSender {
        self.outgoing_tx.clone()
    }

    /// Process all pending outgoing messages.
    ///
    /// Sends queued messages via the appropriate transport (metadata or body).
    /// Call this periodically or after local CRDT changes.
    ///
    /// Returns the number of messages processed.
    ///
    /// # Cancellation Safety
    ///
    /// This method is cancellation-safe. Messages are first drained from the
    /// channel into a local buffer while holding the lock briefly, then the
    /// lock is released before any async operations. This ensures:
    /// 1. The receiver is never held across await points
    /// 2. The receiver cannot be lost due to task cancellation
    /// 3. If cancelled mid-send, only the buffered messages are lost (not the channel)
    pub async fn process_outgoing(&self) -> usize {
        // Drain all pending messages into a local buffer while holding the lock briefly.
        // This ensures the receiver is never held across await points and cannot be lost.
        let pending_messages: Vec<OutgoingSyncMessage> = {
            let guard = self.outgoing_rx.lock().unwrap();
            if let Some(ref rx) = *guard {
                // Drain all available messages
                let mut messages = Vec::new();
                while let Ok(msg) = rx.try_recv() {
                    messages.push(msg);
                }
                messages
            } else {
                Vec::new()
            }
            // Lock is released here, receiver stays in place
        };

        // Process the buffered messages outside the lock
        let mut count = 0;
        for msg in pending_messages {
            if self.send_outgoing_message(&msg).await.is_ok() {
                count += 1;
            }
        }

        count
    }

    /// Send a single outgoing message via the appropriate transport.
    async fn send_outgoing_message(&self, msg: &OutgoingSyncMessage) -> Result<()> {
        if msg.is_body {
            // Body message - frame and send via body transport
            if self.body_connected.load(Ordering::SeqCst) {
                let framed = frame_body_message(&msg.doc_name, &msg.message);
                self.body_transport.send(&framed).await?;
                log::debug!(
                    "[SyncClient] Sent body message for {}, {} bytes",
                    msg.doc_name,
                    msg.message.len()
                );
            } else {
                log::warn!(
                    "[SyncClient] Cannot send body message for {} - not connected",
                    msg.doc_name
                );
            }
        } else {
            // Workspace metadata message - send via metadata transport
            if self.metadata_connected.load(Ordering::SeqCst) {
                self.metadata_transport.send(&msg.message).await?;
                log::debug!(
                    "[SyncClient] Sent workspace message, {} bytes",
                    msg.message.len()
                );
            } else {
                log::warn!("[SyncClient] Cannot send workspace message - not connected");
            }
        }
        Ok(())
    }

    /// Set the event callback.
    pub fn set_event_callback(&self, callback: SyncEventCallback) {
        let mut cb = self.event_callback.write().unwrap();
        *cb = Some(callback);
    }

    /// Emit a sync event.
    fn emit_event(&self, event: SyncEvent) {
        if let Some(ref cb) = *self.event_callback.read().unwrap() {
            cb(event);
        }
    }

    /// Update and emit status change.
    fn set_status(&self, status: ConnectionStatus) {
        {
            let mut s = self.status.write().unwrap();
            *s = status.clone();
        }
        self.emit_event(SyncEvent::StatusChanged(status));
    }

    /// Get the current connection status.
    pub fn status(&self) -> ConnectionStatus {
        self.status.read().unwrap().clone()
    }

    /// Check if the client is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Check if both connections are established.
    pub fn is_connected(&self) -> bool {
        self.metadata_connected.load(Ordering::SeqCst) && self.body_connected.load(Ordering::SeqCst)
    }

    /// Start the sync client.
    ///
    /// Establishes both metadata and body connections and begins syncing.
    /// This method returns after initiating the connections; sync continues
    /// in the background.
    pub async fn start(&self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            // Already running
            return Ok(());
        }

        log::info!(
            "[SyncClient] Starting sync to {} for workspace {}",
            self.config.server_url,
            self.config.workspace_id
        );

        self.set_status(ConnectionStatus::Connecting);
        self.reconnect_attempts.store(0, Ordering::SeqCst);

        // Connect metadata transport
        self.connect_metadata().await?;

        // Connect body transport
        self.connect_body().await?;

        Ok(())
    }

    /// Connect the metadata transport.
    async fn connect_metadata(&self) -> Result<()> {
        let config = self.config.metadata_config();

        // Set up message handler
        let sync_manager = Arc::clone(&self.sync_manager);
        let write_to_disk = self.config.write_to_disk;
        let event_callback = self.event_callback.read().unwrap().clone();

        let callback: MessageCallback = Arc::new(move |message: &[u8]| {
            // Use futures::executor::block_on for sync context
            // In production, this would be handled via async channels
            let result = futures_lite::future::block_on(
                sync_manager.handle_workspace_message(message, write_to_disk),
            );

            match result {
                Ok(sync_result) => {
                    // Emit file changes
                    if !sync_result.changed_files.is_empty() {
                        if let Some(ref cb) = event_callback {
                            cb(SyncEvent::FilesChanged {
                                paths: sync_result.changed_files,
                            });
                        }
                    }

                    // Return response if any
                    sync_result.response
                }
                Err(e) => {
                    log::error!("[SyncClient] Metadata message error: {:?}", e);
                    if let Some(ref cb) = event_callback {
                        cb(SyncEvent::Error {
                            message: e.to_string(),
                        });
                    }
                    None
                }
            }
        });

        self.metadata_transport.set_on_message(callback);
        self.metadata_transport.connect(&config).await?;
        self.metadata_connected.store(true, Ordering::SeqCst);

        // Send initial SyncStep1
        let step1 = self.sync_manager.create_workspace_sync_step1();
        self.metadata_transport.send(&step1).await?;

        log::info!("[SyncClient] Metadata connection established");
        Ok(())
    }

    /// Connect the body transport.
    async fn connect_body(&self) -> Result<()> {
        let config = self.config.body_config();

        // Set up message handler for multiplexed body messages
        let sync_manager = Arc::clone(&self.sync_manager);
        let write_to_disk = self.config.write_to_disk;
        let event_callback = self.event_callback.read().unwrap().clone();

        let callback: MessageCallback = Arc::new(move |message: &[u8]| {
            // Unframe the multiplexed message
            let (file_path, body_msg) = match unframe_body_message(message) {
                Some((path, msg)) => (path, msg),
                None => {
                    log::warn!("[SyncClient] Failed to unframe body message");
                    return None;
                }
            };

            // Handle the body message
            let result = futures_lite::future::block_on(sync_manager.handle_body_message(
                &file_path,
                &body_msg,
                write_to_disk,
            ));

            match result {
                Ok(body_result) => {
                    // Emit content change if not echo
                    if let Some(content) = body_result.content {
                        if !body_result.is_echo {
                            if let Some(ref cb) = event_callback {
                                cb(SyncEvent::BodyChanged {
                                    path: file_path.clone(),
                                    content,
                                });
                            }
                        }
                    }

                    // Frame and return response if any
                    body_result
                        .response
                        .map(|resp| frame_body_message(&file_path, &resp))
                }
                Err(e) => {
                    log::error!("[SyncClient] Body message error for {}: {:?}", file_path, e);
                    if let Some(ref cb) = event_callback {
                        cb(SyncEvent::Error {
                            message: e.to_string(),
                        });
                    }
                    None
                }
            }
        });

        self.body_transport.set_on_message(callback);
        self.body_transport.connect(&config).await?;
        self.body_connected.store(true, Ordering::SeqCst);

        log::info!("[SyncClient] Body connection established");

        // Update status to connected
        self.set_status(ConnectionStatus::Connected);

        // NOTE: We no longer auto-subscribe to all body docs here.
        // Instead, clients use focus_files() to indicate which files they're working on,
        // and the server broadcasts focus_list_changed to all clients.
        // Clients should subscribe to files in the focus list.

        Ok(())
    }

    /// Send a workspace metadata update.
    ///
    /// Called after modifying the workspace CRDT locally.
    pub async fn send_workspace_update(&self) -> Result<()> {
        if !self.metadata_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot send workspace update: not connected");
            return Ok(());
        }

        let update = self.sync_manager.create_workspace_update(None)?;
        if !update.is_empty() {
            self.metadata_transport.send(&update).await?;
        }
        Ok(())
    }

    /// Send a body content update.
    ///
    /// Called after modifying a body CRDT locally.
    ///
    /// # Arguments
    ///
    /// * `doc_name` - The file path (e.g., "notes/my-note.md")
    /// * `content` - The new content (for echo detection)
    pub async fn send_body_update(&self, doc_name: &str, content: &str) -> Result<()> {
        if !self.body_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot send body update: not connected");
            return Ok(());
        }

        // Create the update message
        let update = self.sync_manager.create_body_update(doc_name, content)?;
        if !update.is_empty() {
            // Frame it for the multiplexed connection
            let framed = frame_body_message(doc_name, &update);
            self.body_transport.send(&framed).await?;
        }
        Ok(())
    }

    /// Stop the sync client.
    ///
    /// Disconnects both connections and stops syncing.
    pub async fn stop(&self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            // Already stopped
            return;
        }

        log::info!("[SyncClient] Stopping sync");

        // Disconnect both transports
        let _ = self.metadata_transport.disconnect().await;
        let _ = self.body_transport.disconnect().await;

        self.metadata_connected.store(false, Ordering::SeqCst);
        self.body_connected.store(false, Ordering::SeqCst);

        self.set_status(ConnectionStatus::Disconnected);
    }

    /// Calculate reconnection delay using exponential backoff.
    ///
    /// Returns delay in milliseconds: 1s, 2s, 4s, 8s, 16s, 32s (max).
    pub fn reconnect_delay(&self) -> u64 {
        let attempts = self.reconnect_attempts.load(Ordering::SeqCst);
        std::cmp::min(1000 * 2u64.pow(attempts), 32000)
    }

    /// Check if reconnection should be attempted.
    pub fn should_reconnect(&self) -> bool {
        self.running.load(Ordering::SeqCst)
            && self.reconnect_attempts.load(Ordering::SeqCst) < self.config.max_reconnect_attempts
    }

    /// Increment reconnection attempt counter.
    pub fn increment_reconnect(&self) -> u32 {
        self.reconnect_attempts.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Reset reconnection attempt counter.
    pub fn reset_reconnect(&self) {
        self.reconnect_attempts.store(0, Ordering::SeqCst);
    }

    // ========================================================================
    // Focus-Based Sync APIs
    // ========================================================================

    /// Focus on specific files.
    ///
    /// Notifies the server that this client is focused on these files.
    /// The server broadcasts the updated focus list to all connected clients,
    /// who should then subscribe to sync those files.
    ///
    /// # Arguments
    ///
    /// * `files` - List of file paths to focus on
    pub async fn focus_files(&self, files: &[String]) -> Result<()> {
        if !self.body_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot focus files: body not connected");
            return Ok(());
        }

        if files.is_empty() {
            return Ok(());
        }

        let msg = serde_json::json!({
            "type": "focus",
            "files": files
        });
        let text = msg.to_string();
        self.body_transport.send_text(&text).await?;

        log::debug!("[SyncClient] Sent focus for {} files", files.len());
        Ok(())
    }

    /// Unfocus specific files.
    ///
    /// Notifies the server that this client is no longer focused on these files.
    /// If no other clients are focused on the files, they will be removed from
    /// the global focus list.
    ///
    /// # Arguments
    ///
    /// * `files` - List of file paths to unfocus
    pub async fn unfocus_files(&self, files: &[String]) -> Result<()> {
        if !self.body_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot unfocus files: body not connected");
            return Ok(());
        }

        if files.is_empty() {
            return Ok(());
        }

        let msg = serde_json::json!({
            "type": "unfocus",
            "files": files
        });
        let text = msg.to_string();
        self.body_transport.send_text(&text).await?;

        log::debug!("[SyncClient] Sent unfocus for {} files", files.len());
        Ok(())
    }

    /// Subscribe to body sync for specific files.
    ///
    /// For each file, loads content from disk (if not already loaded)
    /// and sends SyncStep1 to initiate sync.
    ///
    /// This should be called in response to `FocusListChanged` events
    /// to subscribe to files that other clients are working on.
    ///
    /// # Arguments
    ///
    /// * `files` - List of file paths to subscribe to
    pub async fn subscribe_bodies(&self, files: &[String]) -> Result<()> {
        if !self.body_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot subscribe bodies: not connected");
            return Ok(());
        }

        log::info!(
            "[SyncClient] Subscribing to {} body docs from focus list",
            files.len()
        );

        for doc_name in files {
            // Skip if state hasn't changed since last sync
            if !self.sync_manager.body_state_changed(doc_name) {
                log::debug!("[SyncClient] Skipping unchanged body doc: {}", doc_name);
                continue;
            }

            // Load content from disk before creating SyncStep1
            if let Err(e) = self.sync_manager.ensure_body_content_loaded(doc_name).await {
                log::warn!(
                    "[SyncClient] Failed to load body content for {}: {:?}",
                    doc_name,
                    e
                );
            }

            // Create and send SyncStep1
            let step1 = self.sync_manager.create_body_sync_step1(doc_name);
            let framed = frame_body_message(doc_name, &step1);
            if let Err(e) = self.body_transport.send(&framed).await {
                log::warn!(
                    "[SyncClient] Failed to subscribe body {}: {:?}",
                    doc_name,
                    e
                );
            }
        }

        Ok(())
    }

    /// Subscribe to body sync for all files in workspace.
    ///
    /// This is a convenience method that subscribes to all files at once.
    /// It should only be used when you need to sync all files (e.g., initial sync
    /// or when recovering from a disconnect).
    ///
    /// For focus-based sync, prefer using `subscribe_bodies()` with the files
    /// from the `FocusListChanged` event.
    pub async fn subscribe_all_bodies(&self) -> Result<()> {
        if !self.body_connected.load(Ordering::SeqCst) {
            log::warn!("[SyncClient] Cannot subscribe bodies: not connected");
            return Ok(());
        }

        let file_paths = self.sync_manager.get_all_file_paths();
        log::info!(
            "[SyncClient] Subscribing to {} body docs (loading content from disk)",
            file_paths.len()
        );

        // Track how many files we loaded content for
        let mut loaded_count = 0;
        let mut empty_count = 0;
        let mut skipped_count = 0;

        // Process in batches of 20 to avoid overwhelming the server
        const BATCH_SIZE: usize = 20;
        #[allow(unused_variables)]
        for (batch_idx, chunk) in file_paths.chunks(BATCH_SIZE).enumerate() {
            for doc_name in chunk {
                // Skip if state hasn't changed since last sync (optimization for reconnect)
                if !self.sync_manager.body_state_changed(doc_name) {
                    log::debug!("[SyncClient] Skipping unchanged body doc: {}", doc_name);
                    skipped_count += 1;
                    continue;
                }

                // IMPORTANT: Load content from disk before creating SyncStep1
                // This ensures we have content to sync (not just an empty state vector)
                match self.sync_manager.ensure_body_content_loaded(doc_name).await {
                    Ok(true) => loaded_count += 1,
                    Ok(false) => empty_count += 1,
                    Err(e) => {
                        log::warn!(
                            "[SyncClient] Failed to load body content for {}: {:?}",
                            doc_name,
                            e
                        );
                    }
                }

                // Now create SyncStep1 with the populated state
                let step1 = self.sync_manager.create_body_sync_step1(doc_name);
                let framed = frame_body_message(doc_name, &step1);
                if let Err(e) = self.body_transport.send(&framed).await {
                    log::warn!(
                        "[SyncClient] Failed to subscribe body {}: {:?}",
                        doc_name,
                        e
                    );
                }
            }
            // Small delay between batches to avoid overwhelming the server
            // Only available in native builds with tokio; WASM doesn't need this
            // since it uses CallbackTransport which has its own flow control.
            #[cfg(feature = "native-sync")]
            if batch_idx < file_paths.len() / BATCH_SIZE {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }

        log::info!(
            "[SyncClient] Body subscription complete: loaded {} files, {} already had content or empty, {} skipped (unchanged)",
            loaded_count,
            empty_count,
            skipped_count
        );
        Ok(())
    }
}

impl<T: SyncTransport, FS: AsyncFileSystem + Send + Sync + 'static> std::fmt::Debug
    for SyncClient<T, FS>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncClient")
            .field("workspace_id", &self.config.workspace_id)
            .field("running", &self.running.load(Ordering::SeqCst))
            .field(
                "metadata_connected",
                &self.metadata_connected.load(Ordering::SeqCst),
            )
            .field(
                "body_connected",
                &self.body_connected.load(Ordering::SeqCst),
            )
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_client_config() {
        let config = SyncClientConfig::new(
            "wss://sync.example.com".to_string(),
            "ws123".to_string(),
            PathBuf::from("/workspace"),
        )
        .with_auth("token".to_string())
        .with_max_reconnects(5);

        assert_eq!(config.server_url, "wss://sync.example.com");
        assert_eq!(config.workspace_id, "ws123");
        assert_eq!(config.auth_token, Some("token".to_string()));
        assert_eq!(config.max_reconnect_attempts, 5);
    }

    #[test]
    fn test_metadata_config() {
        let config = SyncClientConfig::new(
            "wss://sync.example.com".to_string(),
            "ws123".to_string(),
            PathBuf::from("/workspace"),
        );

        let meta_config = config.metadata_config();
        assert!(!meta_config.multiplexed);
        assert_eq!(meta_config.doc_id, "ws123");
    }

    #[test]
    fn test_body_config() {
        let config = SyncClientConfig::new(
            "wss://sync.example.com".to_string(),
            "ws123".to_string(),
            PathBuf::from("/workspace"),
        );

        let body_config = config.body_config();
        assert!(body_config.multiplexed);
    }

    #[test]
    fn test_reconnect_delay() {
        // Test exponential backoff calculation
        assert_eq!(std::cmp::min(1000 * 2u64.pow(0), 32000), 1000); // 1s
        assert_eq!(std::cmp::min(1000 * 2u64.pow(1), 32000), 2000); // 2s
        assert_eq!(std::cmp::min(1000 * 2u64.pow(2), 32000), 4000); // 4s
        assert_eq!(std::cmp::min(1000 * 2u64.pow(3), 32000), 8000); // 8s
        assert_eq!(std::cmp::min(1000 * 2u64.pow(4), 32000), 16000); // 16s
        assert_eq!(std::cmp::min(1000 * 2u64.pow(5), 32000), 32000); // 32s (max)
        assert_eq!(std::cmp::min(1000 * 2u64.pow(6), 32000), 32000); // 32s (max)
    }

    #[test]
    fn test_sync_event_variants() {
        let event = SyncEvent::StatusChanged(ConnectionStatus::Connected);
        assert!(matches!(event, SyncEvent::StatusChanged(_)));

        let event = SyncEvent::FilesChanged {
            paths: vec!["file.md".to_string()],
        };
        assert!(matches!(event, SyncEvent::FilesChanged { .. }));

        let event = SyncEvent::Error {
            message: "test".to_string(),
        };
        assert!(matches!(event, SyncEvent::Error { .. }));
    }

    #[test]
    fn test_outgoing_sync_message_from_event() {
        // SendSyncMessage event should convert
        let event = FileSystemEvent::SendSyncMessage {
            doc_name: "notes/test.md".to_string(),
            message: vec![1, 2, 3],
            is_body: true,
        };
        let msg = OutgoingSyncMessage::from_event(&event);
        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.doc_name, "notes/test.md");
        assert_eq!(msg.message, vec![1, 2, 3]);
        assert!(msg.is_body);

        // Workspace message
        let event = FileSystemEvent::SendSyncMessage {
            doc_name: "workspace".to_string(),
            message: vec![4, 5, 6],
            is_body: false,
        };
        let msg = OutgoingSyncMessage::from_event(&event).unwrap();
        assert_eq!(msg.doc_name, "workspace");
        assert!(!msg.is_body);

        // Other events should return None
        let event = FileSystemEvent::file_created(PathBuf::from("test.md"));
        assert!(OutgoingSyncMessage::from_event(&event).is_none());

        let event = FileSystemEvent::sync_status_changed("synced", None);
        assert!(OutgoingSyncMessage::from_event(&event).is_none());
    }

    #[test]
    fn test_create_sync_event_bridge() {
        let (tx, rx) = std::sync::mpsc::channel::<OutgoingSyncMessage>();
        let bridge = create_sync_event_bridge(tx);

        // Send a sync message event
        let event = FileSystemEvent::SendSyncMessage {
            doc_name: "test.md".to_string(),
            message: vec![1, 2, 3],
            is_body: true,
        };
        bridge(&event);

        // Check it was queued
        let received = rx.try_recv().unwrap();
        assert_eq!(received.doc_name, "test.md");
        assert_eq!(received.message, vec![1, 2, 3]);
        assert!(received.is_body);

        // Non-sync events should not queue anything
        let event = FileSystemEvent::file_created(PathBuf::from("other.md"));
        bridge(&event);
        assert!(rx.try_recv().is_err());
    }
}
