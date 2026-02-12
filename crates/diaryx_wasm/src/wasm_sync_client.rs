//! WASM sync client using the shared SyncSession protocol handler.
//!
//! This module provides `WasmSyncClient`, a wasm-bindgen wrapper that delegates
//! all protocol logic to `SyncSession` from `diaryx_core`. JavaScript owns the
//! WebSocket and feeds messages in; Rust handles handshake, framing, routing.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     JavaScript Layer                           │
//! │  ┌──────────────────────────────────┐                         │
//! │  │ Single WebSocket to /sync2       │                         │
//! │  └──────────────┬───────────────────┘                         │
//! │                 │ onmessage / send()                          │
//! └─────────────────┼────────────────────────────────────────────┘
//!                   │
//! ┌─────────────────┼────────────────────────────────────────────┐
//! │                 ▼                                            │
//! │  ┌──────────────────────┐                                   │
//! │  │   WasmSyncClient     │  ← inject/poll bridge             │
//! │  │   (this module)      │                                   │
//! │  └──────────┬───────────┘                                   │
//! │             ▼                                                │
//! │  ┌──────────────────────┐                                   │
//! │  │     SyncSession      │  ← shared protocol handler        │
//! │  │   (diaryx_core)      │    handshake, framing, routing    │
//! │  └──────────┬───────────┘                                   │
//! │             ▼                                                │
//! │  ┌──────────────────────┐                                   │
//! │  │   RustSyncManager    │  ← Y-sync protocol, CRDT ops     │
//! │  └──────────────────────┘                                   │
//! │                 Rust/WASM Layer                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! const client = backend.createSyncClient(serverUrl, workspaceId, authToken);
//! const ws = new WebSocket(client.getWsUrl());
//! ws.binaryType = 'arraybuffer';
//!
//! ws.onopen = async () => {
//!   await client.onConnected();
//!   drainOutgoing(ws, client);
//! };
//!
//! ws.onmessage = async (e) => {
//!   if (typeof e.data === 'string') await client.onTextMessage(e.data);
//!   else await client.onBinaryMessage(new Uint8Array(e.data));
//!   drainOutgoing(ws, client);
//! };
//!
//! ws.onclose = async () => { await client.onDisconnected(); };
//!
//! function drainOutgoing(ws, client) {
//!   let msg;
//!   while ((msg = client.pollOutgoingBinary())) ws.send(msg);
//!   while ((msg = client.pollOutgoingText())) ws.send(msg);
//! }
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use diaryx_core::crdt::{
    IncomingEvent, RustSyncManager, SessionAction, SyncSession, SyncSessionConfig,
};
use diaryx_core::fs::{CrdtFs, EventEmittingFs};
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::backend::StorageBackend;

/// WASM sync client backed by the shared SyncSession protocol handler.
///
/// JavaScript feeds WebSocket events in via `onConnected()`, `onBinaryMessage()`,
/// etc. The client processes them through `SyncSession` and queues outgoing
/// messages/events for JavaScript to poll.
#[wasm_bindgen]
pub struct WasmSyncClient {
    /// The shared protocol handler (handshake, framing, routing).
    session: SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>,

    /// Queue of outgoing binary messages (framed v2 protocol).
    outgoing_binary: RefCell<VecDeque<Vec<u8>>>,

    /// Queue of outgoing text messages (JSON control messages).
    outgoing_text: RefCell<VecDeque<String>>,

    /// Queue of JSON-serialized SyncEvent objects for JS.
    events: RefCell<VecDeque<String>>,

    /// Server URL (e.g., "https://sync.diaryx.org").
    server_url: String,

    /// Workspace ID.
    workspace_id: String,

    /// Auth token (optional).
    auth_token: Option<String>,

    /// Session code for share sessions (optional).
    session_code: Option<String>,
}

impl WasmSyncClient {
    /// Create a new WasmSyncClient.
    ///
    /// Called internally by `DiaryxBackend::createSyncClient()`.
    pub(crate) fn new(
        server_url: String,
        workspace_id: String,
        auth_token: Option<String>,
        session_config: SyncSessionConfig,
        sync_manager: Arc<RustSyncManager<EventEmittingFs<CrdtFs<StorageBackend>>>>,
    ) -> Self {
        let session = SyncSession::new(session_config, Arc::clone(&sync_manager));

        Self {
            session,
            outgoing_binary: RefCell::new(VecDeque::new()),
            outgoing_text: RefCell::new(VecDeque::new()),
            events: RefCell::new(VecDeque::new()),
            server_url,
            workspace_id,
            auth_token,
            session_code: None,
        }
    }
}

#[wasm_bindgen]
impl WasmSyncClient {
    // =========================================================================
    // Connection URL
    // =========================================================================

    /// Get the WebSocket URL for the v2 sync connection.
    ///
    /// Returns a URL like `wss://sync.example.com/sync2?token=...&session=...`
    #[wasm_bindgen(js_name = "getWsUrl")]
    pub fn get_ws_url(&self) -> String {
        let ws_server = self
            .server_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");

        let mut url = format!("{}/sync2", ws_server);
        let mut params = Vec::new();

        if let Some(ref token) = self.auth_token {
            params.push(format!("token={}", token));
        }
        if let Some(ref code) = self.session_code {
            params.push(format!("session={}", code));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        url
    }

    /// Set a share session code.
    ///
    /// Call this before connecting to join a share session.
    #[wasm_bindgen(js_name = "setSessionCode")]
    pub fn set_session_code(&mut self, code: String) {
        self.session_code = Some(code);
    }

    // =========================================================================
    // Event Injection — JavaScript feeds WebSocket events
    // =========================================================================

    /// Notify that the WebSocket connected.
    ///
    /// Triggers workspace SyncStep1 and handshake. After calling this,
    /// poll `pollOutgoingBinary()` / `pollOutgoingText()` to get messages to send.
    #[wasm_bindgen(js_name = "onConnected")]
    pub fn on_connected(&self) -> Promise {
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        // SAFETY: WASM is single-threaded, so this raw pointer dereference is safe.
        // The SyncSession lives as long as WasmSyncClient, and we're inside a method on it.
        let outgoing_binary = &self.outgoing_binary as *const RefCell<VecDeque<Vec<u8>>>;
        let outgoing_text = &self.outgoing_text as *const RefCell<VecDeque<String>>;
        let events = &self.events as *const RefCell<VecDeque<String>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session.process(IncomingEvent::Connected).await;
            let outgoing_binary = unsafe { &*outgoing_binary };
            let outgoing_text = unsafe { &*outgoing_text };
            let events = unsafe { &*events };

            for action in actions {
                match action {
                    SessionAction::SendBinary(data) => {
                        outgoing_binary.borrow_mut().push_back(data);
                    }
                    SessionAction::SendText(text) => {
                        outgoing_text.borrow_mut().push_back(text);
                    }
                    SessionAction::Emit(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            events.borrow_mut().push_back(json);
                        }
                    }
                    SessionAction::DownloadSnapshot { workspace_id } => {
                        let event = serde_json::json!({
                            "type": "downloadSnapshot",
                            "workspaceId": workspace_id,
                        });
                        events.borrow_mut().push_back(event.to_string());
                    }
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Inject an incoming binary WebSocket message.
    ///
    /// Returns a Promise that resolves when processing is complete.
    /// After this, poll outgoing queues.
    #[wasm_bindgen(js_name = "onBinaryMessage")]
    pub fn on_binary_message(&self, data: &[u8]) -> Promise {
        let data = data.to_vec();
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        let outgoing_binary = &self.outgoing_binary as *const RefCell<VecDeque<Vec<u8>>>;
        let outgoing_text = &self.outgoing_text as *const RefCell<VecDeque<String>>;
        let events = &self.events as *const RefCell<VecDeque<String>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session.process(IncomingEvent::BinaryMessage(data)).await;
            let outgoing_binary = unsafe { &*outgoing_binary };
            let outgoing_text = unsafe { &*outgoing_text };
            let events = unsafe { &*events };

            for action in actions {
                match action {
                    SessionAction::SendBinary(data) => {
                        outgoing_binary.borrow_mut().push_back(data);
                    }
                    SessionAction::SendText(text) => {
                        outgoing_text.borrow_mut().push_back(text);
                    }
                    SessionAction::Emit(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            events.borrow_mut().push_back(json);
                        }
                    }
                    SessionAction::DownloadSnapshot { workspace_id } => {
                        let event = serde_json::json!({
                            "type": "downloadSnapshot",
                            "workspaceId": workspace_id,
                        });
                        events.borrow_mut().push_back(event.to_string());
                    }
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Inject an incoming text WebSocket message (JSON control message).
    ///
    /// Returns a Promise that resolves when processing is complete.
    #[wasm_bindgen(js_name = "onTextMessage")]
    pub fn on_text_message(&self, text: String) -> Promise {
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        let outgoing_binary = &self.outgoing_binary as *const RefCell<VecDeque<Vec<u8>>>;
        let outgoing_text_q = &self.outgoing_text as *const RefCell<VecDeque<String>>;
        let events = &self.events as *const RefCell<VecDeque<String>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session.process(IncomingEvent::TextMessage(text)).await;
            let outgoing_binary = unsafe { &*outgoing_binary };
            let outgoing_text_q = unsafe { &*outgoing_text_q };
            let events = unsafe { &*events };

            for action in actions {
                match action {
                    SessionAction::SendBinary(data) => {
                        outgoing_binary.borrow_mut().push_back(data);
                    }
                    SessionAction::SendText(text) => {
                        outgoing_text_q.borrow_mut().push_back(text);
                    }
                    SessionAction::Emit(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            events.borrow_mut().push_back(json);
                        }
                    }
                    SessionAction::DownloadSnapshot { workspace_id } => {
                        let event = serde_json::json!({
                            "type": "downloadSnapshot",
                            "workspaceId": workspace_id,
                        });
                        events.borrow_mut().push_back(event.to_string());
                    }
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Notify that a snapshot was downloaded and imported.
    ///
    /// Call this after handling a `downloadSnapshot` event.
    #[wasm_bindgen(js_name = "onSnapshotImported")]
    pub fn on_snapshot_imported(&self) -> Promise {
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        let outgoing_binary = &self.outgoing_binary as *const RefCell<VecDeque<Vec<u8>>>;
        let outgoing_text = &self.outgoing_text as *const RefCell<VecDeque<String>>;
        let events = &self.events as *const RefCell<VecDeque<String>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session.process(IncomingEvent::SnapshotImported).await;
            let outgoing_binary = unsafe { &*outgoing_binary };
            let outgoing_text = unsafe { &*outgoing_text };
            let events = unsafe { &*events };

            for action in actions {
                match action {
                    SessionAction::SendBinary(data) => {
                        outgoing_binary.borrow_mut().push_back(data);
                    }
                    SessionAction::SendText(text) => {
                        outgoing_text.borrow_mut().push_back(text);
                    }
                    SessionAction::Emit(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            events.borrow_mut().push_back(json);
                        }
                    }
                    _ => {}
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Notify that the WebSocket disconnected.
    #[wasm_bindgen(js_name = "onDisconnected")]
    pub fn on_disconnected(&self) -> Promise {
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        let events = &self.events as *const RefCell<VecDeque<String>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session.process(IncomingEvent::Disconnected).await;
            let events = unsafe { &*events };

            for action in actions {
                if let SessionAction::Emit(event) = action {
                    if let Ok(json) = serde_json::to_string(&event) {
                        events.borrow_mut().push_back(json);
                    }
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Queue a local CRDT update for sending to the server.
    ///
    /// Call this when local CRDT changes need to be synced.
    #[wasm_bindgen(js_name = "queueLocalUpdate")]
    pub fn queue_local_update(&self, doc_id: &str, data: &[u8]) -> Promise {
        let doc_id = doc_id.to_string();
        let data = data.to_vec();
        let session_ptr =
            &self.session as *const SyncSession<EventEmittingFs<CrdtFs<StorageBackend>>>;
        let outgoing_binary = &self.outgoing_binary as *const RefCell<VecDeque<Vec<u8>>>;

        future_to_promise(async move {
            let session = unsafe { &*session_ptr };
            let actions = session
                .process(IncomingEvent::LocalUpdate { doc_id, data })
                .await;
            let outgoing_binary = unsafe { &*outgoing_binary };

            for action in actions {
                if let SessionAction::SendBinary(data) = action {
                    outgoing_binary.borrow_mut().push_back(data);
                }
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    // =========================================================================
    // Message Polling — JavaScript polls for outgoing data
    // =========================================================================

    /// Poll for an outgoing binary message.
    ///
    /// Returns a Uint8Array if there's a message to send, null otherwise.
    #[wasm_bindgen(js_name = "pollOutgoingBinary")]
    pub fn poll_outgoing_binary(&self) -> Option<js_sys::Uint8Array> {
        self.outgoing_binary
            .borrow_mut()
            .pop_front()
            .map(|msg| js_sys::Uint8Array::from(msg.as_slice()))
    }

    /// Poll for an outgoing text message.
    ///
    /// Returns a string if there's a message to send, null otherwise.
    #[wasm_bindgen(js_name = "pollOutgoingText")]
    pub fn poll_outgoing_text(&self) -> Option<String> {
        self.outgoing_text.borrow_mut().pop_front()
    }

    /// Poll for a JSON-serialized event.
    ///
    /// Returns a JSON string representing a SyncEvent, or null.
    #[wasm_bindgen(js_name = "pollEvent")]
    pub fn poll_event(&self) -> Option<String> {
        self.events.borrow_mut().pop_front()
    }

    /// Check if there are pending outgoing messages or events.
    #[wasm_bindgen(js_name = "hasOutgoing")]
    pub fn has_outgoing(&self) -> bool {
        !self.outgoing_binary.borrow().is_empty() || !self.outgoing_text.borrow().is_empty()
    }

    /// Check if there are pending events.
    #[wasm_bindgen(js_name = "hasEvents")]
    pub fn has_events(&self) -> bool {
        !self.events.borrow().is_empty()
    }

    // =========================================================================
    // Status
    // =========================================================================

    /// Get the workspace ID.
    #[wasm_bindgen(js_name = "getWorkspaceId")]
    pub fn get_workspace_id(&self) -> String {
        self.workspace_id.clone()
    }

    /// Get the server URL.
    #[wasm_bindgen(js_name = "getServerUrl")]
    pub fn get_server_url(&self) -> String {
        self.server_url.clone()
    }

    // =========================================================================
    // Focus API
    // =========================================================================

    /// Send a focus message for specific files.
    ///
    /// Other clients will be notified which files this client is interested in.
    #[wasm_bindgen(js_name = "focusFiles")]
    pub fn focus_files(&self, files: Vec<String>) {
        let msg = serde_json::json!({
            "type": "focus",
            "files": files,
        });
        self.outgoing_text.borrow_mut().push_back(msg.to_string());
    }

    /// Send an unfocus message for specific files.
    #[wasm_bindgen(js_name = "unfocusFiles")]
    pub fn unfocus_files(&self, files: Vec<String>) {
        let msg = serde_json::json!({
            "type": "unfocus",
            "files": files,
        });
        self.outgoing_text.borrow_mut().push_back(msg.to_string());
    }
}
