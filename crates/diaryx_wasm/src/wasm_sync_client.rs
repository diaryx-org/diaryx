//! WASM-specific sync client wrapper for JavaScript integration.
//!
//! This module provides `WasmSyncClient`, a wasm-bindgen wrapper around
//! `SyncClient<CallbackTransport>`. It exposes the sync client's functionality
//! to JavaScript while keeping all sync protocol logic in Rust.
//!
//! ## Architecture
//!
//! Unlike native clients that own WebSocket connections via tokio-tungstenite,
//! WASM clients use a callback-based architecture where:
//!
//! 1. **JavaScript owns the WebSockets**: JS creates and manages WebSocket connections
//! 2. **Rust owns the sync logic**: All Y-sync protocol handling happens in Rust
//! 3. **CallbackTransport bridges them**: Messages flow via inject/poll pattern
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     JavaScript Layer                             │
//! │  ┌────────────────────┐       ┌────────────────────┐            │
//! │  │ Metadata WebSocket │       │ Body WebSocket     │            │
//! │  └─────────┬──────────┘       └─────────┬──────────┘            │
//! │            │ onmessage                  │ onmessage             │
//! │            │ send()                     │ send()                │
//! └────────────┼────────────────────────────┼───────────────────────┘
//!              │                            │
//! ┌────────────┼────────────────────────────┼───────────────────────┐
//! │            ▼                            ▼                       │
//! │  ┌──────────────────┐       ┌──────────────────┐                │
//! │  │CallbackTransport │       │CallbackTransport │                │
//! │  │  (metadata)      │       │  (body)          │                │
//! │  └────────┬─────────┘       └────────┬─────────┘                │
//! │           │                          │                          │
//! │           └────────────┬─────────────┘                          │
//! │                        ▼                                        │
//! │              ┌──────────────────┐                               │
//! │              │  WasmSyncClient  │                               │
//! │              │  (this module)   │                               │
//! │              └────────┬─────────┘                               │
//! │                       ▼                                         │
//! │              ┌──────────────────┐                               │
//! │              │ RustSyncManager  │                               │
//! │              └──────────────────┘                               │
//! │                 Rust/WASM Layer                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! // Create sync client from backend
//! const client = backend.createSyncClient(serverUrl, workspaceId, authToken);
//!
//! // Create and connect WebSockets
//! const metaUrl = client.getMetadataUrl();
//! const bodyUrl = client.getBodyUrl();
//!
//! const metaWs = new WebSocket(metaUrl);
//! metaWs.binaryType = 'arraybuffer';
//! metaWs.onopen = () => client.markMetadataConnected();
//! metaWs.onclose = () => client.markMetadataDisconnected();
//! metaWs.onmessage = async (e) => {
//!   const response = await client.injectMetadataMessage(new Uint8Array(e.data));
//!   if (response) metaWs.send(response);
//! };
//!
//! // Similar for body WebSocket...
//!
//! // Start polling for outgoing messages
//! const pollInterval = setInterval(() => {
//!   let msg;
//!   while ((msg = client.pollMetadataOutgoing())) metaWs.send(msg);
//!   while ((msg = client.pollBodyOutgoing())) bodyWs.send(msg);
//! }, 50);
//!
//! // Start sync (sends initial SyncStep1 messages)
//! await client.start();
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use diaryx_core::crdt::{RustSyncManager, SyncClientConfig, SyncConfig, SyncTransport};
use diaryx_core::fs::{CrdtFs, EventEmittingFs};
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::backend::StorageBackend;
use crate::callback_transport::CallbackTransport;

/// WASM sync client wrapper for JavaScript integration.
///
/// This struct wraps a `SyncClient<CallbackTransport>` and exposes its
/// functionality to JavaScript via wasm-bindgen. It manages two transports
/// (metadata and body) and provides methods for:
///
/// - Getting WebSocket URLs for connection
/// - Notifying connection status changes
/// - Injecting incoming messages
/// - Polling for outgoing messages
/// - Starting and stopping sync
#[wasm_bindgen]
pub struct WasmSyncClient {
    /// Transport for metadata (workspace) sync.
    metadata_transport: Rc<RefCell<CallbackTransport>>,

    /// Transport for body (content) sync.
    body_transport: Rc<RefCell<CallbackTransport>>,

    /// Sync configuration.
    config: SyncClientConfig,

    /// Whether sync has been started.
    started: RefCell<bool>,

    /// Reference to the sync manager for creating sync messages.
    sync_manager: Arc<RustSyncManager<EventEmittingFs<CrdtFs<StorageBackend>>>>,
}

impl WasmSyncClient {
    /// Create a new WasmSyncClient.
    ///
    /// This is called internally by `DiaryxBackend::createSyncClient()`.
    pub fn new(
        config: SyncClientConfig,
        sync_manager: Arc<RustSyncManager<EventEmittingFs<CrdtFs<StorageBackend>>>>,
    ) -> Self {
        let metadata_transport = Rc::new(RefCell::new(CallbackTransport::new()));
        let body_transport = Rc::new(RefCell::new(CallbackTransport::new()));

        Self {
            metadata_transport,
            body_transport,
            config,
            started: RefCell::new(false),
            sync_manager,
        }
    }
}

#[wasm_bindgen]
impl WasmSyncClient {
    // =========================================================================
    // URL Getters - JavaScript uses these to create WebSocket connections
    // =========================================================================

    /// Get the WebSocket URL for the metadata connection.
    ///
    /// Returns null if sync hasn't been configured.
    #[wasm_bindgen(js_name = "getMetadataUrl")]
    pub fn get_metadata_url(&self) -> Option<String> {
        let config = SyncConfig::metadata(
            self.config.server_url.clone(),
            self.config.workspace_id.clone(),
        );
        let config = if let Some(ref token) = self.config.auth_token {
            config.with_auth(token.clone())
        } else {
            config
        };
        Some(config.build_url())
    }

    /// Get the WebSocket URL for the body connection.
    ///
    /// Returns null if sync hasn't been configured.
    #[wasm_bindgen(js_name = "getBodyUrl")]
    pub fn get_body_url(&self) -> Option<String> {
        let config = SyncConfig::body(
            self.config.server_url.clone(),
            self.config.workspace_id.clone(),
        );
        let config = if let Some(ref token) = self.config.auth_token {
            config.with_auth(token.clone())
        } else {
            config
        };
        Some(config.build_url())
    }

    // =========================================================================
    // Connection Status - JavaScript notifies when WebSocket state changes
    // =========================================================================

    /// Mark the metadata connection as connected.
    ///
    /// Call this when the metadata WebSocket opens.
    #[wasm_bindgen(js_name = "markMetadataConnected")]
    pub fn mark_metadata_connected(&self) {
        self.metadata_transport.borrow().mark_connected();
        log::info!("[WasmSyncClient] Metadata connection marked as connected");
    }

    /// Mark the metadata connection as disconnected.
    ///
    /// Call this when the metadata WebSocket closes.
    #[wasm_bindgen(js_name = "markMetadataDisconnected")]
    pub fn mark_metadata_disconnected(&self) {
        self.metadata_transport.borrow().mark_disconnected();
        log::info!("[WasmSyncClient] Metadata connection marked as disconnected");
    }

    /// Mark the body connection as connected.
    ///
    /// Call this when the body WebSocket opens.
    #[wasm_bindgen(js_name = "markBodyConnected")]
    pub fn mark_body_connected(&self) {
        self.body_transport.borrow().mark_connected();
        log::info!("[WasmSyncClient] Body connection marked as connected");
    }

    /// Mark the body connection as disconnected.
    ///
    /// Call this when the body WebSocket closes.
    #[wasm_bindgen(js_name = "markBodyDisconnected")]
    pub fn mark_body_disconnected(&self) {
        self.body_transport.borrow().mark_disconnected();
        log::info!("[WasmSyncClient] Body connection marked as disconnected");
    }

    // =========================================================================
    // Message Injection - JavaScript forwards received WebSocket messages
    // =========================================================================

    /// Inject an incoming metadata message.
    ///
    /// Call this when the metadata WebSocket receives a message.
    /// Returns a Promise that resolves to a Uint8Array response (or null).
    ///
    /// The response should be sent back via the WebSocket if not null.
    #[wasm_bindgen(js_name = "injectMetadataMessage")]
    pub fn inject_metadata_message(&self, message: &[u8]) -> Promise {
        let sync_manager = Arc::clone(&self.sync_manager);
        let message = message.to_vec();
        let write_to_disk = self.config.write_to_disk;

        future_to_promise(async move {
            match sync_manager
                .handle_workspace_message(&message, write_to_disk)
                .await
            {
                Ok(result) => {
                    log::debug!(
                        "[WasmSyncClient] Handled metadata message: {} changed files",
                        result.changed_files.len()
                    );
                    match result.response {
                        Some(resp) => Ok(js_sys::Uint8Array::from(resp.as_slice()).into()),
                        None => Ok(JsValue::NULL),
                    }
                }
                Err(e) => {
                    log::error!("[WasmSyncClient] Metadata message error: {:?}", e);
                    Err(JsValue::from_str(&format!("Sync error: {}", e)))
                }
            }
        })
    }

    /// Inject an incoming body message.
    ///
    /// Call this when the body WebSocket receives a message.
    /// The message should already be unframed (doc_name extracted separately).
    /// Returns a Promise that resolves to a Uint8Array response (or null).
    #[wasm_bindgen(js_name = "injectBodyMessage")]
    pub fn inject_body_message(&self, doc_name: &str, message: &[u8]) -> Promise {
        let sync_manager = Arc::clone(&self.sync_manager);
        let doc_name = doc_name.to_string();
        let message = message.to_vec();
        let write_to_disk = self.config.write_to_disk;

        future_to_promise(async move {
            match sync_manager
                .handle_body_message(&doc_name, &message, write_to_disk)
                .await
            {
                Ok(result) => {
                    log::debug!(
                        "[WasmSyncClient] Handled body message for {}: content_changed={}",
                        doc_name,
                        result.content.is_some()
                    );
                    match result.response {
                        Some(resp) => Ok(js_sys::Uint8Array::from(resp.as_slice()).into()),
                        None => Ok(JsValue::NULL),
                    }
                }
                Err(e) => {
                    log::error!(
                        "[WasmSyncClient] Body message error for {}: {:?}",
                        doc_name,
                        e
                    );
                    Err(JsValue::from_str(&format!("Body sync error: {}", e)))
                }
            }
        })
    }

    // =========================================================================
    // Message Polling - JavaScript polls for outgoing messages to send
    // =========================================================================

    /// Poll for an outgoing metadata message.
    ///
    /// Returns a Uint8Array if there's a message to send, null otherwise.
    /// JavaScript should call this in a polling loop and send any messages
    /// via the metadata WebSocket.
    #[wasm_bindgen(js_name = "pollMetadataOutgoing")]
    pub fn poll_metadata_outgoing(&self) -> Option<js_sys::Uint8Array> {
        self.metadata_transport
            .borrow()
            .poll_outgoing()
            .map(|msg| js_sys::Uint8Array::from(msg.as_slice()))
    }

    /// Poll for an outgoing body message.
    ///
    /// Returns a Uint8Array if there's a message to send, null otherwise.
    /// The message is already framed with the document name.
    #[wasm_bindgen(js_name = "pollBodyOutgoing")]
    pub fn poll_body_outgoing(&self) -> Option<js_sys::Uint8Array> {
        self.body_transport
            .borrow()
            .poll_outgoing()
            .map(|msg| js_sys::Uint8Array::from(msg.as_slice()))
    }

    /// Check if there are pending metadata outgoing messages.
    #[wasm_bindgen(js_name = "hasMetadataOutgoing")]
    pub fn has_metadata_outgoing(&self) -> bool {
        self.metadata_transport.borrow().has_outgoing()
    }

    /// Check if there are pending body outgoing messages.
    #[wasm_bindgen(js_name = "hasBodyOutgoing")]
    pub fn has_body_outgoing(&self) -> bool {
        self.body_transport.borrow().has_outgoing()
    }

    // =========================================================================
    // Sync Lifecycle
    // =========================================================================

    /// Start the sync session.
    ///
    /// This should be called after both WebSocket connections are established.
    /// It sends the initial SyncStep1 messages and subscribes to all body docs.
    ///
    /// Returns a Promise that resolves when initial sync messages are sent.
    #[wasm_bindgen]
    pub fn start(&self) -> Promise {
        let sync_manager = Arc::clone(&self.sync_manager);
        let metadata_transport = Rc::clone(&self.metadata_transport);
        let body_transport = Rc::clone(&self.body_transport);
        let started = self.started.clone();

        future_to_promise(async move {
            // Prevent double-start
            if *started.borrow() {
                log::warn!("[WasmSyncClient] Already started");
                return Ok(JsValue::UNDEFINED);
            }
            *started.borrow_mut() = true;

            log::info!("[WasmSyncClient] Starting sync session");

            // Send workspace SyncStep1
            let step1 = sync_manager.create_workspace_sync_step1();
            metadata_transport.borrow().queue_message(step1);
            log::debug!("[WasmSyncClient] Queued workspace SyncStep1");

            // Subscribe to all body docs (loading content from disk first)
            let file_paths = sync_manager.get_all_file_paths();
            log::info!(
                "[WasmSyncClient] Subscribing to {} body docs (loading content from disk)",
                file_paths.len()
            );

            let mut loaded_count = 0;
            let mut empty_count = 0;

            for doc_name in file_paths {
                // IMPORTANT: Load content from disk before creating SyncStep1
                // This ensures we have content to sync (not just an empty state vector)
                match sync_manager.ensure_body_content_loaded(&doc_name).await {
                    Ok(true) => loaded_count += 1,
                    Ok(false) => empty_count += 1,
                    Err(e) => {
                        log::warn!(
                            "[WasmSyncClient] Failed to load body content for {}: {:?}",
                            doc_name,
                            e
                        );
                    }
                }

                // Now create SyncStep1 with the populated state
                let step1 = sync_manager.create_body_sync_step1(&doc_name);
                let framed = diaryx_core::crdt::frame_body_message(&doc_name, &step1);
                body_transport.borrow().queue_message(framed);
            }

            log::info!(
                "[WasmSyncClient] Body subscription complete: loaded {} files, {} already had content or empty",
                loaded_count,
                empty_count
            );

            log::info!("[WasmSyncClient] Sync session started");
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Stop the sync session.
    ///
    /// Clears all pending messages and resets state.
    #[wasm_bindgen]
    pub fn stop(&self) {
        log::info!("[WasmSyncClient] Stopping sync session");

        self.metadata_transport.borrow().clear_outgoing();
        self.body_transport.borrow().clear_outgoing();
        *self.started.borrow_mut() = false;

        log::info!("[WasmSyncClient] Sync session stopped");
    }

    // =========================================================================
    // Status
    // =========================================================================

    /// Check if the sync client is running.
    #[wasm_bindgen(js_name = "isRunning")]
    pub fn is_running(&self) -> bool {
        *self.started.borrow()
    }

    /// Check if the metadata connection is established.
    #[wasm_bindgen(js_name = "isMetadataConnected")]
    pub fn is_metadata_connected(&self) -> bool {
        self.metadata_transport.borrow().is_connected()
    }

    /// Check if the body connection is established.
    #[wasm_bindgen(js_name = "isBodyConnected")]
    pub fn is_body_connected(&self) -> bool {
        self.body_transport.borrow().is_connected()
    }

    /// Check if both connections are established.
    #[wasm_bindgen(js_name = "isConnected")]
    pub fn is_connected(&self) -> bool {
        self.is_metadata_connected() && self.is_body_connected()
    }

    /// Get the workspace ID.
    #[wasm_bindgen(js_name = "getWorkspaceId")]
    pub fn get_workspace_id(&self) -> String {
        self.config.workspace_id.clone()
    }

    /// Get the server URL.
    #[wasm_bindgen(js_name = "getServerUrl")]
    pub fn get_server_url(&self) -> String {
        self.config.server_url.clone()
    }

    // =========================================================================
    // Manual Sync Triggers
    // =========================================================================

    /// Get the initial SyncStep1 message for workspace sync.
    ///
    /// Returns a Uint8Array containing the message to send via WebSocket.
    #[wasm_bindgen(js_name = "getWorkspaceSyncStep1")]
    pub fn get_workspace_sync_step1(&self) -> js_sys::Uint8Array {
        let step1 = self.sync_manager.create_workspace_sync_step1();
        js_sys::Uint8Array::from(step1.as_slice())
    }

    /// Get the initial SyncStep1 message for a body document.
    ///
    /// Returns a Uint8Array containing the framed message to send via WebSocket.
    #[wasm_bindgen(js_name = "getBodySyncStep1")]
    pub fn get_body_sync_step1(&self, doc_name: &str) -> js_sys::Uint8Array {
        let step1 = self.sync_manager.create_body_sync_step1(doc_name);
        let framed = diaryx_core::crdt::frame_body_message(doc_name, &step1);
        js_sys::Uint8Array::from(framed.as_slice())
    }

    /// Subscribe to body sync for a specific document.
    ///
    /// This queues a SyncStep1 message for the given document.
    /// Call this when a new file is created or when opening a file for editing.
    #[wasm_bindgen(js_name = "subscribeBody")]
    pub fn subscribe_body(&self, doc_name: &str) {
        let step1 = self.sync_manager.create_body_sync_step1(doc_name);
        let framed = diaryx_core::crdt::frame_body_message(doc_name, &step1);
        self.body_transport.borrow().queue_message(framed);
        log::debug!("[WasmSyncClient] Subscribed to body: {}", doc_name);
    }

    /// Queue a workspace update message for sending.
    ///
    /// This creates a Y-sync Update message from the current workspace state
    /// and queues it for sending via the metadata WebSocket.
    #[wasm_bindgen(js_name = "queueWorkspaceUpdate")]
    pub fn queue_workspace_update(&self) -> std::result::Result<(), JsValue> {
        match self.sync_manager.create_workspace_update(None) {
            Ok(update) => {
                if !update.is_empty() {
                    self.metadata_transport.borrow().queue_message(update);
                    log::debug!("[WasmSyncClient] Queued workspace update");
                }
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&format!(
                "Failed to create workspace update: {}",
                e
            ))),
        }
    }

    /// Queue a body update message for sending.
    ///
    /// This creates a Y-sync Update message for the given document
    /// and queues it for sending via the body WebSocket.
    #[wasm_bindgen(js_name = "queueBodyUpdate")]
    pub fn queue_body_update(
        &self,
        doc_name: &str,
        content: &str,
    ) -> std::result::Result<(), JsValue> {
        match self.sync_manager.create_body_update(doc_name, content) {
            Ok(update) => {
                if !update.is_empty() {
                    let framed = diaryx_core::crdt::frame_body_message(doc_name, &update);
                    self.body_transport.borrow().queue_message(framed);
                    log::debug!("[WasmSyncClient] Queued body update for {}", doc_name);
                }
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&format!(
                "Failed to create body update: {}",
                e
            ))),
        }
    }

    // =========================================================================
    // Focus API - Focus-based sync subscription
    // =========================================================================

    /// Focus on specific files for sync.
    ///
    /// Sends a focus message to the server indicating which files the client
    /// is currently interested in syncing. Other clients will receive a
    /// `focus_list_changed` notification and can subscribe to sync updates
    /// for these files.
    ///
    /// Call this when a file is opened in the editor.
    ///
    /// ## Example
    /// ```javascript
    /// // User opens a file
    /// client.focusFiles(["workspace/notes.md"]);
    /// ```
    #[wasm_bindgen(js_name = "focusFiles")]
    pub fn focus_files(&self, files: Vec<String>) {
        let focus_msg = serde_json::json!({
            "type": "focus",
            "files": files
        });
        self.body_transport
            .borrow()
            .queue_outgoing_text(focus_msg.to_string());
        log::debug!(
            "[WasmSyncClient] Queued focus message for {} files",
            files.len()
        );
    }

    /// Unfocus specific files.
    ///
    /// Sends an unfocus message to the server indicating the client is no
    /// longer interested in syncing these files.
    ///
    /// Call this when a file is closed in the editor.
    ///
    /// ## Example
    /// ```javascript
    /// // User closes a file
    /// client.unfocusFiles(["workspace/notes.md"]);
    /// ```
    #[wasm_bindgen(js_name = "unfocusFiles")]
    pub fn unfocus_files(&self, files: Vec<String>) {
        let unfocus_msg = serde_json::json!({
            "type": "unfocus",
            "files": files
        });
        self.body_transport
            .borrow()
            .queue_outgoing_text(unfocus_msg.to_string());
        log::debug!(
            "[WasmSyncClient] Queued unfocus message for {} files",
            files.len()
        );
    }

    /// Poll for an outgoing body text message (for focus/unfocus).
    ///
    /// Returns a string if there's a text message to send, null otherwise.
    /// JavaScript should call this in a polling loop and send any messages
    /// via the body WebSocket as text frames.
    #[wasm_bindgen(js_name = "pollBodyOutgoingText")]
    pub fn poll_body_outgoing_text(&self) -> Option<String> {
        self.body_transport.borrow().poll_outgoing_text()
    }

    /// Check if there are pending body outgoing text messages.
    #[wasm_bindgen(js_name = "hasBodyOutgoingText")]
    pub fn has_body_outgoing_text(&self) -> bool {
        self.body_transport.borrow().has_outgoing_text()
    }

    /// Subscribe to body sync for the currently focused files.
    ///
    /// This sends SyncStep1 messages for all files in the provided list.
    /// Call this after receiving a `focus_list_changed` message from the server.
    ///
    /// ## Example
    /// ```javascript
    /// // Received focus_list_changed event
    /// const files = event.files;
    /// client.subscribeBodies(files);
    /// ```
    #[wasm_bindgen(js_name = "subscribeBodies")]
    pub fn subscribe_bodies(&self, files: Vec<String>) {
        for doc_name in &files {
            let step1 = self.sync_manager.create_body_sync_step1(doc_name);
            let framed = diaryx_core::crdt::frame_body_message(doc_name, &step1);
            self.body_transport.borrow().queue_message(framed);
        }
        log::debug!("[WasmSyncClient] Subscribed to {} body docs", files.len());
    }
}

// Helper trait to add queue_message to CallbackTransport
trait QueueMessage {
    fn queue_message(&self, message: Vec<u8>);
}

impl QueueMessage for CallbackTransport {
    fn queue_message(&self, message: Vec<u8>) {
        // CallbackTransport's queue_outgoing is now public and synchronous.
        // It just pushes to an internal RefCell<VecDeque>.
        self.queue_outgoing(message);
    }
}
