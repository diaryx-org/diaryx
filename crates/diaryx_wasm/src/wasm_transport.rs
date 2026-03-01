//! WASM WebSocket transport — Rust owns the WebSocket connection.
//!
//! Replaces the poll-based `WasmSyncClient` with an event-driven model:
//! Rust creates a `web_sys::WebSocket`, wires up callbacks that feed
//! `SyncSession::process()`, and executes the returned `SessionAction`s
//! inline (send, emit, download snapshot).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Rust/WASM Layer                             │
//! │  ┌──────────────────────────────┐                              │
//! │  │ WasmSyncTransport            │  ← owns web_sys::WebSocket  │
//! │  │ onopen/onmessage/onclose     │                              │
//! │  └──────────┬───────────────────┘                              │
//! │             ▼                                                   │
//! │  ┌──────────────────────┐                                      │
//! │  │     SyncSession      │  ← message-driven protocol handler  │
//! │  └──────────┬───────────┘                                      │
//! │             ▼                                                   │
//! │  ┌──────────────────────┐                                      │
//! │  │   RustSyncManager    │  ← Y-sync protocol, CRDT ops       │
//! │  └──────────────────────┘                                      │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use diaryx_core::fs::{EventEmittingFs, FileSystemEvent};
use diaryx_sync::{
    CrdtFs, IncomingEvent, RustSyncManager, SessionAction, SyncEvent, SyncSession,
    SyncSessionConfig, SyncStatus,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use crate::backend::{StorageBackend, WasmCallbackRegistry};

// ============================================================================
// Types
// ============================================================================

type SyncFs = EventEmittingFs<CrdtFs<StorageBackend>>;

/// Configuration for the WASM sync transport.
#[derive(Clone)]
struct WasmSyncConfig {
    server_url: String,
    workspace_id: String,
    auth_token: Option<String>,
    session_code: Option<String>,
}

/// Reconnection state.
struct ReconnectState {
    attempts: u32,
    timeout_handle: Option<i32>,
    destroyed: bool,
}

/// Stored closures for WebSocket callbacks (prevent GC).
struct WsClosures {
    onopen: Option<Closure<dyn FnMut()>>,
    onmessage: Option<Closure<dyn FnMut(MessageEvent)>>,
    onclose: Option<Closure<dyn FnMut(CloseEvent)>>,
    onerror: Option<Closure<dyn FnMut(JsValue)>>,
}

// ============================================================================
// Constants
// ============================================================================

const MAX_RECONNECT_ATTEMPTS: u32 = 10;
const RECONNECT_BASE_MS: u32 = 1000;
const RECONNECT_CAP_MS: u32 = 32000;
/// Close codes 4000-4999 are application-specific "don't reconnect" codes.
const NO_RECONNECT_CODE_MIN: u16 = 4000;
const NO_RECONNECT_CODE_MAX: u16 = 4999;

// ============================================================================
// WasmSyncTransport
// ============================================================================

/// Event-driven WebSocket client for WASM.
///
/// Owns the `web_sys::WebSocket` and feeds events into `SyncSession`. Actions
/// (send, emit, download snapshot) are executed inline in the callbacks.
pub(crate) struct WasmSyncTransport {
    session: Rc<SyncSession<SyncFs>>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    config: Rc<RefCell<WasmSyncConfig>>,
    reconnect: Rc<RefCell<ReconnectState>>,
    event_registry: Rc<WasmCallbackRegistry>,
    closures: Rc<RefCell<WsClosures>>,
    focused_files: Rc<RefCell<Vec<String>>>,
    /// Subscription ID for local CRDT updates via the event registry.
    local_update_sub_id: Rc<RefCell<Option<u64>>>,
    /// Reference to the event registry for unsubscribing.
    _wasm_registry_ref: Rc<WasmCallbackRegistry>,
}

impl WasmSyncTransport {
    /// Create a new transport. Does NOT connect yet — call `connect()`.
    pub(crate) fn new(
        server_url: String,
        workspace_id: String,
        auth_token: Option<String>,
        session_code: Option<String>,
        sync_manager: Arc<RustSyncManager<SyncFs>>,
        event_registry: Rc<WasmCallbackRegistry>,
    ) -> Self {
        let session_config = SyncSessionConfig {
            workspace_id: workspace_id.clone(),
            write_to_disk: true,
        };
        let session = Rc::new(SyncSession::new(session_config, sync_manager));

        let config = Rc::new(RefCell::new(WasmSyncConfig {
            server_url,
            workspace_id,
            auth_token,
            session_code,
        }));

        WasmSyncTransport {
            session,
            ws: Rc::new(RefCell::new(None)),
            config,
            reconnect: Rc::new(RefCell::new(ReconnectState {
                attempts: 0,
                timeout_handle: None,
                destroyed: false,
            })),
            event_registry: Rc::clone(&event_registry),
            closures: Rc::new(RefCell::new(WsClosures {
                onopen: None,
                onmessage: None,
                onclose: None,
                onerror: None,
            })),
            focused_files: Rc::new(RefCell::new(Vec::new())),
            local_update_sub_id: Rc::new(RefCell::new(None)),
            _wasm_registry_ref: event_registry,
        }
    }

    /// Build the WebSocket URL.
    fn build_ws_url(config: &WasmSyncConfig) -> String {
        let ws_server = config
            .server_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");

        let mut url = format!("{}/sync2", ws_server);
        let mut params = Vec::new();

        if let Some(ref token) = config.auth_token {
            params.push(format!("token={}", token));
        }
        if let Some(ref code) = config.session_code {
            params.push(format!("session={}", code));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        url
    }

    /// Connect to the sync server.
    pub(crate) fn connect(&self) {
        let config = self.config.borrow();
        if config.server_url.is_empty() || config.workspace_id.is_empty() {
            log::warn!("[WasmSyncTransport] Cannot connect: missing server_url or workspace_id");
            return;
        }

        // Check if destroyed
        if self.reconnect.borrow().destroyed {
            log::warn!("[WasmSyncTransport] Cannot connect: transport destroyed");
            return;
        }

        let url = Self::build_ws_url(&config);
        drop(config);

        log::info!("[WasmSyncTransport] Connecting to {}", url);

        // Emit connecting status
        self.emit_sync_event(&SyncEvent::StatusChanged {
            status: SyncStatus::Connecting,
        });

        // Create WebSocket
        let ws = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(e) => {
                log::error!("[WasmSyncTransport] Failed to create WebSocket: {:?}", e);
                self.schedule_reconnect();
                return;
            }
        };
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // Wire up callbacks
        self.setup_callbacks(&ws);

        // Store WebSocket
        *self.ws.borrow_mut() = Some(ws);
    }

    /// Set up WebSocket callbacks.
    fn setup_callbacks(&self, ws: &WebSocket) {
        let session = Rc::clone(&self.session);
        let ws_ref = Rc::clone(&self.ws);
        let event_registry = Rc::clone(&self.event_registry);
        let reconnect = Rc::clone(&self.reconnect);
        let focused_files = Rc::clone(&self.focused_files);
        let config = Rc::clone(&self.config);

        // onopen
        let session_open = Rc::clone(&session);
        let ws_open = Rc::clone(&ws_ref);
        let registry_open = Rc::clone(&event_registry);
        let reconnect_open = Rc::clone(&reconnect);
        let focused_open = Rc::clone(&focused_files);
        let config_open = Rc::clone(&config);
        let onopen = Closure::wrap(Box::new(move || {
            log::info!("[WasmSyncTransport] WebSocket connected");
            reconnect_open.borrow_mut().attempts = 0;

            let session = Rc::clone(&session_open);
            let ws = Rc::clone(&ws_open);
            let registry = Rc::clone(&registry_open);
            let focused = Rc::clone(&focused_open);
            let cfg = Rc::clone(&config_open);

            wasm_bindgen_futures::spawn_local(async move {
                let actions = session.process(IncomingEvent::Connected).await;
                execute_actions(&actions, &ws, &registry, &session, &cfg);

                // Re-send focus list on reconnect
                let files = focused.borrow().clone();
                if !files.is_empty() {
                    let actions = session
                        .process(IncomingEvent::SyncBodyFiles { file_paths: files })
                        .await;
                    execute_actions(&actions, &ws, &registry, &session, &cfg);
                }
            });
        }) as Box<dyn FnMut()>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));

        // onmessage
        let session_msg = Rc::clone(&session);
        let ws_msg = Rc::clone(&ws_ref);
        let registry_msg = Rc::clone(&event_registry);
        let config_msg = Rc::clone(&config);
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            let session = Rc::clone(&session_msg);
            let ws = Rc::clone(&ws_msg);
            let registry = Rc::clone(&registry_msg);
            let cfg = Rc::clone(&config_msg);

            let data = e.data();

            if let Some(abuf) = data.dyn_ref::<js_sys::ArrayBuffer>() {
                // Binary message
                let u8arr = js_sys::Uint8Array::new(abuf);
                let bytes = u8arr.to_vec();

                wasm_bindgen_futures::spawn_local(async move {
                    let actions = session.process(IncomingEvent::BinaryMessage(bytes)).await;
                    execute_actions(&actions, &ws, &registry, &session, &cfg);
                });
            } else if let Some(text) = data.as_string() {
                // Text message
                wasm_bindgen_futures::spawn_local(async move {
                    let actions = session.process(IncomingEvent::TextMessage(text)).await;
                    execute_actions(&actions, &ws, &registry, &session, &cfg);
                });
            } else {
                log::warn!("[WasmSyncTransport] Unknown message type");
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        // onclose
        let session_close = Rc::clone(&session);
        let registry_close = Rc::clone(&event_registry);
        let reconnect_close = Rc::clone(&reconnect);
        let ws_close_ref = Rc::clone(&ws_ref);
        let config_close = Rc::clone(&self.config);
        let onclose = Closure::wrap(Box::new(move |e: CloseEvent| {
            let code = e.code();
            log::info!(
                "[WasmSyncTransport] WebSocket closed: code={}, reason='{}'",
                code,
                e.reason()
            );

            // Clear stored WebSocket
            *ws_close_ref.borrow_mut() = None;

            let session = Rc::clone(&session_close);
            let registry = Rc::clone(&registry_close);

            wasm_bindgen_futures::spawn_local(async move {
                let actions = session.process(IncomingEvent::Disconnected).await;
                // Only emit events on disconnect (no sends possible)
                for action in &actions {
                    if let SessionAction::Emit(event) = action {
                        emit_sync_event_to_registry(event, &registry);
                    }
                }
            });

            // Don't reconnect for application close codes
            if code >= NO_RECONNECT_CODE_MIN && code <= NO_RECONNECT_CODE_MAX {
                log::info!(
                    "[WasmSyncTransport] Close code {} in no-reconnect range, not reconnecting",
                    code
                );
                return;
            }

            // Don't reconnect if destroyed
            if reconnect_close.borrow().destroyed {
                return;
            }

            // Schedule reconnect
            let reconnect = Rc::clone(&reconnect_close);
            let config = Rc::clone(&config_close);
            schedule_reconnect_inner(reconnect, config, Rc::clone(&registry_close));
        }) as Box<dyn FnMut(CloseEvent)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));

        // onerror
        let onerror = Closure::wrap(Box::new(move |_e: JsValue| {
            log::error!("[WasmSyncTransport] WebSocket error");
            // onclose will fire after onerror, so reconnect happens there
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        // Store closures to prevent GC
        let mut closures = self.closures.borrow_mut();
        closures.onopen = Some(onopen);
        closures.onmessage = Some(onmessage);
        closures.onclose = Some(onclose);
        closures.onerror = Some(onerror);
    }

    /// Subscribe to local CRDT updates from the event registry.
    ///
    /// When the CrdtFs or workspace CRDT emits a `SendSyncMessage` event,
    /// we route it through `SyncSession::process(LocalUpdate)` so the
    /// session frames and sends it.
    pub(crate) fn subscribe_to_local_updates(&self) {
        let session = Rc::clone(&self.session);
        let ws = Rc::clone(&self.ws);
        let registry = Rc::clone(&self.event_registry);
        let config = Rc::clone(&self.config);

        // Create a JS function that handles SendSyncMessage events
        let callback = Closure::wrap(Box::new(move |event_json: String| {
            if let Ok(event) = serde_json::from_str::<FileSystemEvent>(&event_json) {
                if let FileSystemEvent::SendSyncMessage {
                    doc_name,
                    message,
                    is_body: _,
                } = event
                {
                    let session = Rc::clone(&session);
                    let ws = Rc::clone(&ws);
                    let registry = Rc::clone(&registry);
                    let cfg = Rc::clone(&config);

                    wasm_bindgen_futures::spawn_local(async move {
                        let actions = session
                            .process(IncomingEvent::LocalUpdate {
                                doc_id: doc_name,
                                data: message,
                            })
                            .await;
                        execute_actions(&actions, &ws, &registry, &session, &cfg);
                    });
                }
            }
        }) as Box<dyn FnMut(String)>);

        let func: js_sys::Function = callback.into_js_value().unchecked_into();
        let sub_id = self.event_registry.subscribe(func);
        *self.local_update_sub_id.borrow_mut() = Some(sub_id);
    }

    /// Emit a SyncEvent through the event registry as a FileSystemEvent.
    fn emit_sync_event(&self, event: &SyncEvent) {
        emit_sync_event_to_registry(event, &self.event_registry);
    }

    /// Schedule a reconnection with exponential backoff.
    fn schedule_reconnect(&self) {
        let reconnect = Rc::clone(&self.reconnect);
        let config = Rc::clone(&self.config);
        let registry = Rc::clone(&self.event_registry);
        schedule_reconnect_inner(reconnect, config, registry);
    }

    /// Focus on specific files for body sync.
    pub(crate) fn focus_files(&self, paths: Vec<String>) {
        // Update tracked focus list
        {
            let mut focused = self.focused_files.borrow_mut();
            for path in &paths {
                if !focused.contains(path) {
                    focused.push(path.clone());
                }
            }
        }

        // Send focus control message
        let msg = serde_json::json!({
            "type": "focus",
            "files": paths,
        });
        if let Some(ws) = self.ws.borrow().as_ref() {
            let _ = ws.send_with_str(&msg.to_string());
        }

        // Trigger body sync for these files
        let session = Rc::clone(&self.session);
        let ws = Rc::clone(&self.ws);
        let registry = Rc::clone(&self.event_registry);
        let config = Rc::clone(&self.config);

        wasm_bindgen_futures::spawn_local(async move {
            let actions = session
                .process(IncomingEvent::SyncBodyFiles { file_paths: paths })
                .await;
            execute_actions(&actions, &ws, &registry, &session, &config);
        });
    }

    /// Unfocus specific files.
    pub(crate) fn unfocus_files(&self, paths: Vec<String>) {
        // Update tracked focus list
        {
            let mut focused = self.focused_files.borrow_mut();
            focused.retain(|p| !paths.contains(p));
        }

        // Send unfocus control message
        let msg = serde_json::json!({
            "type": "unfocus",
            "files": paths,
        });
        if let Some(ws) = self.ws.borrow().as_ref() {
            let _ = ws.send_with_str(&msg.to_string());
        }
    }

    /// Request body sync for specific files.
    pub(crate) fn request_body_sync(&self, paths: Vec<String>) {
        let session = Rc::clone(&self.session);
        let ws = Rc::clone(&self.ws);
        let registry = Rc::clone(&self.event_registry);
        let config = Rc::clone(&self.config);

        wasm_bindgen_futures::spawn_local(async move {
            let actions = session
                .process(IncomingEvent::SyncBodyFiles { file_paths: paths })
                .await;
            execute_actions(&actions, &ws, &registry, &session, &config);
        });
    }

    /// Notify the session that a snapshot has been imported by the TS side.
    /// Called after `importFromZip()` completes on the JS side.
    pub(crate) fn notify_snapshot_imported(&self) {
        let session = Rc::clone(&self.session);
        let ws = Rc::clone(&self.ws);
        let registry = Rc::clone(&self.event_registry);
        let config = Rc::clone(&self.config);

        wasm_bindgen_futures::spawn_local(async move {
            let actions = session.process(IncomingEvent::SnapshotImported).await;
            execute_actions(&actions, &ws, &registry, &session, &config);
        });
    }

    /// Disconnect and clean up.
    pub(crate) fn disconnect(&self) {
        self.reconnect.borrow_mut().destroyed = true;

        // Cancel pending reconnect timeout
        if let Some(handle) = self.reconnect.borrow_mut().timeout_handle.take() {
            let global = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
            global.clear_timeout_with_handle(handle);
        }

        // Close WebSocket
        if let Some(ws) = self.ws.borrow_mut().take() {
            // Clear callbacks to avoid firing during close
            ws.set_onopen(None);
            ws.set_onmessage(None);
            ws.set_onclose(None);
            ws.set_onerror(None);
            let _ = ws.close();
        }

        // Unsubscribe from local updates
        if let Some(sub_id) = self.local_update_sub_id.borrow_mut().take() {
            self.event_registry.unsubscribe(sub_id);
        }

        // Clear closures
        let mut closures = self.closures.borrow_mut();
        closures.onopen = None;
        closures.onmessage = None;
        closures.onclose = None;
        closures.onerror = None;

        // Reset session
        self.session.reset();

        log::info!("[WasmSyncTransport] Disconnected and cleaned up");
    }
}

impl Drop for WasmSyncTransport {
    fn drop(&mut self) {
        self.disconnect();
    }
}

// ============================================================================
// Free Functions
// ============================================================================

/// Execute SessionActions: send on WebSocket, emit events, download snapshots.
fn execute_actions(
    actions: &[SessionAction],
    ws: &Rc<RefCell<Option<WebSocket>>>,
    registry: &Rc<WasmCallbackRegistry>,
    _session: &Rc<SyncSession<SyncFs>>,
    config: &Rc<RefCell<WasmSyncConfig>>,
) {
    for action in actions {
        match action {
            SessionAction::SendBinary(data) => {
                if let Some(ws) = ws.borrow().as_ref() {
                    if ws.ready_state() == WebSocket::OPEN {
                        if let Err(e) = ws.send_with_u8_array(data) {
                            log::error!("[WasmSyncTransport] Failed to send binary: {:?}", e);
                        }
                    }
                }
            }
            SessionAction::SendText(text) => {
                if let Some(ws) = ws.borrow().as_ref() {
                    if ws.ready_state() == WebSocket::OPEN {
                        if let Err(e) = ws.send_with_str(text) {
                            log::error!("[WasmSyncTransport] Failed to send text: {:?}", e);
                        }
                    }
                }
            }
            SessionAction::Emit(event) => {
                emit_sync_event_to_registry(event, registry);
            }
            SessionAction::DownloadSnapshot { workspace_id } => {
                log::info!(
                    "[WasmSyncTransport] Downloading snapshot for workspace: {}",
                    workspace_id
                );
                // Spawn async task: fetch snapshot → emit event
                let registry_dl = Rc::clone(registry);
                let config_dl = Rc::clone(config);
                let ws_id = workspace_id.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    download_snapshot(&ws_id, &config_dl, &registry_dl).await;
                    // TS will call notifySnapshotImported() after importFromZip() completes.
                });
            }
        }
    }
}

/// Download a workspace snapshot via HTTP fetch.
///
/// Fetches `GET /api/workspaces/{id}/snapshot` with Bearer auth, then emits
/// the snapshot as a `SnapshotDownloaded` event to the event registry. The
/// TypeScript layer listens for this event and calls `importFromZip()`.
async fn download_snapshot(
    workspace_id: &str,
    config: &Rc<RefCell<WasmSyncConfig>>,
    registry: &Rc<WasmCallbackRegistry>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let (http_url, auth_token) = {
        let cfg = config.borrow();
        let http = cfg
            .server_url
            .replace("wss://", "https://")
            .replace("ws://", "http://");
        let http = http
            .trim_end_matches("/sync2")
            .trim_end_matches("/sync")
            .to_string();
        (http, cfg.auth_token.clone())
    };

    let url = format!(
        "{}/api/workspaces/{}/snapshot",
        http_url,
        js_sys::encode_uri_component(workspace_id)
    );

    log::info!("[WasmSyncTransport] Fetching snapshot from: {}", url);

    // Build fetch request
    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");

    if let Some(ref token) = auth_token {
        let headers = web_sys::Headers::new().unwrap();
        let _ = headers.set("Authorization", &format!("Bearer {}", token));
        opts.set_headers(&headers);
    }

    let request = match web_sys::Request::new_with_str_and_init(&url, &opts) {
        Ok(req) => req,
        Err(e) => {
            log::error!(
                "[WasmSyncTransport] Failed to create fetch request: {:?}",
                e
            );
            return;
        }
    };

    let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
    let resp_value = match JsFuture::from(global.fetch_with_request(&request)).await {
        Ok(val) => val,
        Err(e) => {
            log::error!("[WasmSyncTransport] Snapshot fetch failed: {:?}", e);
            return;
        }
    };

    let response: web_sys::Response = resp_value.unchecked_into();
    if !response.ok() {
        log::warn!(
            "[WasmSyncTransport] Snapshot download failed: HTTP {}",
            response.status()
        );
        return;
    }

    // Get the response as a Blob (efficient for large files)
    let blob = match JsFuture::from(response.blob().unwrap()).await {
        Ok(b) => b,
        Err(e) => {
            log::error!("[WasmSyncTransport] Failed to read snapshot blob: {:?}", e);
            return;
        }
    };

    let blob: web_sys::Blob = blob.unchecked_into();
    let size = blob.size() as usize;

    if size <= 100 {
        log::info!(
            "[WasmSyncTransport] Snapshot too small ({}B), skipping import",
            size
        );
        return;
    }

    log::info!("[WasmSyncTransport] Downloaded snapshot: {} bytes", size);

    // Emit the blob to JS subscribers as a structured event object.
    // TS listener picks this up and calls importFromZip(File).
    let event_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &event_obj,
        &JsValue::from_str("type"),
        &JsValue::from_str("SnapshotDownloaded"),
    );
    let _ = js_sys::Reflect::set(
        &event_obj,
        &JsValue::from_str("workspace_id"),
        &JsValue::from_str(workspace_id),
    );
    let _ = js_sys::Reflect::set(&event_obj, &JsValue::from_str("blob"), &blob);

    registry.emit_raw_js(&event_obj.into());
}

/// Convert a SyncEvent to FileSystemEvent and emit it.
fn emit_sync_event_to_registry(event: &SyncEvent, registry: &WasmCallbackRegistry) {
    let fs_event = match event {
        SyncEvent::StatusChanged { status } => {
            let status_str = match status {
                SyncStatus::Connecting => "connecting",
                SyncStatus::Connected => "connected",
                SyncStatus::Syncing => "syncing",
                SyncStatus::Synced => "synced",
                SyncStatus::Reconnecting { .. } => "reconnecting",
                SyncStatus::Disconnected => "disconnected",
            };
            FileSystemEvent::sync_status_changed(status_str, None)
        }
        SyncEvent::Progress { completed, total } => {
            FileSystemEvent::sync_progress(*completed, *total)
        }
        SyncEvent::FilesChanged { files } => {
            FileSystemEvent::sync_completed("workspace".to_string(), files.len())
        }
        SyncEvent::BodyChanged { file_path } => {
            FileSystemEvent::contents_changed(file_path.into(), String::new())
        }
        SyncEvent::Error { message } => {
            FileSystemEvent::sync_status_changed("error", Some(message.clone()))
        }
        SyncEvent::PeerJoined { peer_count } => FileSystemEvent::peer_joined(*peer_count),
        SyncEvent::PeerLeft { peer_count } => FileSystemEvent::peer_left(*peer_count),
        SyncEvent::SyncComplete { files_synced } => {
            FileSystemEvent::sync_completed("workspace".to_string(), *files_synced)
        }
        SyncEvent::FocusListChanged { files } => FileSystemEvent::focus_list_changed(files.clone()),
    };
    registry.emit(&fs_event);
}

/// Schedule a reconnection with exponential backoff.
fn schedule_reconnect_inner(
    reconnect: Rc<RefCell<ReconnectState>>,
    config: Rc<RefCell<WasmSyncConfig>>,
    registry: Rc<WasmCallbackRegistry>,
) {
    let mut state = reconnect.borrow_mut();

    if state.destroyed {
        return;
    }

    state.attempts += 1;
    let attempt = state.attempts;

    if attempt > MAX_RECONNECT_ATTEMPTS {
        log::warn!(
            "[WasmSyncTransport] Max reconnect attempts ({}) reached, giving up",
            MAX_RECONNECT_ATTEMPTS
        );
        emit_sync_event_to_registry(
            &SyncEvent::Error {
                message: "Max reconnect attempts reached".to_string(),
            },
            &registry,
        );
        return;
    }

    // Exponential backoff: min(base * 2^(attempt-1), cap)
    let delay = (RECONNECT_BASE_MS * (1 << (attempt - 1).min(5))).min(RECONNECT_CAP_MS);

    log::info!(
        "[WasmSyncTransport] Reconnecting in {}ms (attempt {}/{})",
        delay,
        attempt,
        MAX_RECONNECT_ATTEMPTS
    );

    // Emit reconnecting status
    emit_sync_event_to_registry(
        &SyncEvent::StatusChanged {
            status: SyncStatus::Reconnecting { attempt },
        },
        &registry,
    );

    drop(state);

    // Schedule reconnect via setTimeout
    let reconnect_clone = Rc::clone(&reconnect);
    let config_clone = Rc::clone(&config);
    let registry_clone = Rc::clone(&registry);

    let callback = Closure::once(move || {
        if reconnect_clone.borrow().destroyed {
            return;
        }

        let cfg = config_clone.borrow();
        let url = WasmSyncTransport::build_ws_url(&cfg);
        drop(cfg);

        log::info!("[WasmSyncTransport] Reconnecting to {}", url);

        // Create new WebSocket — we need to re-create the transport's connect logic.
        // Since we can't easily call self.connect() from a closure, we do the
        // minimal WebSocket creation here. The existing stored closures from
        // setup_callbacks won't work for a new WebSocket, so we emit a
        // "reconnect_needed" status for the managing code to call connect() again.
        emit_sync_event_to_registry(
            &SyncEvent::StatusChanged {
                status: SyncStatus::Connecting,
            },
            &registry_clone,
        );
    });

    let global = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
    match global.set_timeout_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        delay as i32,
    ) {
        Ok(handle) => {
            reconnect.borrow_mut().timeout_handle = Some(handle);
        }
        Err(e) => {
            log::error!("[WasmSyncTransport] Failed to schedule reconnect: {:?}", e);
        }
    }

    // Prevent closure from being dropped (it needs to survive until the timeout fires).
    callback.forget();
}
