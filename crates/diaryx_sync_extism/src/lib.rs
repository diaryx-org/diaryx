//! Extism guest plugin wrapping diaryx_sync for on-demand CRDT sync.
//!
//! This crate compiles to a `.wasm` module loaded by the Extism host runtime
//! (wasmtime on native, @extism/extism JS SDK on web). It owns all CRDT state
//! (WorkspaceCrdt, BodyDocManager) in its own WASM sandbox and exposes both
//! JSON-based and binary-native exports.
//!
//! ## JSON exports (standard Extism protocol)
//!
//! - `manifest()` — plugin metadata + UI contributions
//! - `init()` — initialize with workspace config
//! - `shutdown()` — persist state and clean up
//! - `handle_command()` — structured commands (sync state, CRDT ops, etc.)
//! - `on_event()` — filesystem events from the host
//! - `get_config()` / `set_config()` — plugin configuration
//!
//! ## Binary exports (hot path)
//!
//! - `handle_binary_message()` — framed v2 sync message in, action list out
//! - `handle_text_message()` — control/handshake messages
//! - `on_connected()` — connection established, returns initial sync messages
//! - `on_disconnected()` — connection lost
//! - `queue_local_update()` — local CRDT change, returns sync messages to send
//! - `drain()` — poll outgoing messages + events

pub mod binary_protocol;
pub mod host_bridge;
pub mod host_fs;
pub mod state;

// Custom getrandom backends for the Extism WASM guest.
//
// The default browser backends require wasm-bindgen imports (crypto.getRandomValues)
// which aren't available in the Extism wasmtime runtime. We provide custom
// implementations seeded from the host timestamp for both getrandom 0.2 and 0.4.
mod custom_random {
    use std::sync::atomic::{AtomicU64, Ordering};

    static RNG_STATE: AtomicU64 = AtomicU64::new(0);

    fn xorshift_fill(buf: &mut [u8]) {
        let mut state = RNG_STATE.load(Ordering::Relaxed);
        if state == 0 {
            state = crate::host_bridge::get_timestamp().unwrap_or(42);
            if state == 0 {
                state = 42;
            }
        }
        for byte in buf.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        RNG_STATE.store(state, Ordering::Relaxed);
    }

    // getrandom 0.2 custom backend (used by fastrand, futures-lite)
    fn custom_getrandom_v02(buf: &mut [u8]) -> Result<(), getrandom::Error> {
        xorshift_fill(buf);
        Ok(())
    }

    getrandom::register_custom_getrandom!(custom_getrandom_v02);

    // getrandom 0.3 custom backend (used by uuid/rng-getrandom).
    // The `getrandom_backend="custom"` cfg (set in .cargo/config.toml) tells
    // getrandom 0.3 to call this extern function instead of using browser JS APIs.
    #[unsafe(no_mangle)]
    unsafe extern "Rust" fn __getrandom_v03_custom(
        dest: *mut u8,
        len: usize,
    ) -> Result<(), getrandom_03::Error> {
        unsafe {
            let buf = core::slice::from_raw_parts_mut(dest, len);
            xorshift_fill(buf);
        }
        Ok(())
    }
}

use extism_pdk::*;
use serde_json::Value as JsonValue;

use diaryx_core::plugin::{ComponentRef, SettingsField, SidebarSide, UiContribution};
use diaryx_sync::IncomingEvent;

// Re-export the protocol types from diaryx_extism for compatibility
// (we define compatible types here since diaryx_extism is a host-side crate)

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestManifest {
    id: String,
    name: String,
    version: String,
    description: String,
    capabilities: Vec<String>,
    #[serde(default)]
    ui: Vec<JsonValue>,
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestEvent {
    event_type: String,
    payload: JsonValue,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandRequest {
    command: String,
    params: JsonValue,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandResponse {
    success: bool,
    #[serde(default)]
    data: Option<JsonValue>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    write_to_disk: Option<bool>,
}

// ============================================================================
// JSON exports
// ============================================================================

/// Return the plugin manifest (metadata + UI contributions).
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let sync_settings_tab = UiContribution::SettingsTab {
        id: "sync-settings".into(),
        label: "Sync".into(),
        icon: None,
        fields: vec![
            SettingsField::Section {
                label: "Sync Configuration".into(),
                description: Some("Configure real-time synchronization across devices".into()),
            },
            SettingsField::Toggle {
                key: "enabled".into(),
                label: "Enable Sync".into(),
                description: Some("Sync your workspace across devices in real-time".into()),
            },
        ],
    };

    let share_tab = UiContribution::SidebarTab {
        id: "share".into(),
        label: "Share".into(),
        icon: Some("share".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Builtin {
            component_id: "sync.share".into(),
        },
    };

    let snapshots_tab = UiContribution::SidebarTab {
        id: "snapshots".into(),
        label: "Snapshots".into(),
        icon: Some("history".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Builtin {
            component_id: "sync.snapshots".into(),
        },
    };

    let history_tab = UiContribution::SidebarTab {
        id: "history".into(),
        label: "History".into(),
        icon: Some("history".into()),
        side: SidebarSide::Right,
        component: ComponentRef::Builtin {
            component_id: "sync.history".into(),
        },
    };

    let status_bar_item = UiContribution::StatusBarItem {
        id: "sync-status".into(),
        label: "Sync".into(),
        position: diaryx_core::plugin::StatusBarPosition::Right,
        plugin_command: Some("get_sync_status".into()),
    };

    let manifest = GuestManifest {
        id: "sync".into(),
        name: "Sync".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Real-time CRDT sync across devices".into(),
        capabilities: vec![
            "workspace_events".into(),
            "file_events".into(),
            "crdt_commands".into(),
            "sync_transport".into(),
            "custom_commands".into(),
        ],
        ui: vec![
            serde_json::to_value(&sync_settings_tab).unwrap_or_default(),
            serde_json::to_value(&share_tab).unwrap_or_default(),
            serde_json::to_value(&snapshots_tab).unwrap_or_default(),
            serde_json::to_value(&history_tab).unwrap_or_default(),
            serde_json::to_value(&status_bar_item).unwrap_or_default(),
        ],
        commands: all_commands(),
    };

    Ok(serde_json::to_string(&manifest)?)
}

/// Initialize the plugin with workspace configuration.
#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or(InitParams {
        workspace_root: None,
        workspace_id: None,
        write_to_disk: None,
    });

    state::init_state(params.workspace_id.clone()).map_err(extism_pdk::Error::msg)?;

    // If workspace_root is provided, configure the sync handler
    if let Some(root) = &params.workspace_root {
        let init_result = state::with_state(|s| {
            let ctx = diaryx_core::plugin::PluginContext {
                workspace_root: Some(std::path::PathBuf::from(root)),
                link_format: diaryx_core::link_parser::LinkFormat::default(),
            };
            // block_on the async init
            poll_future(diaryx_core::plugin::Plugin::init(&s.sync_plugin, &ctx))
                .map_err(|e| format!("Plugin init failed: {e}"))
        })
        .map_err(|e| extism_pdk::Error::msg(e))?;
        init_result.map_err(extism_pdk::Error::msg)?;
    }

    // If workspace_id provided, create a session
    if let Some(ws_id) = &params.workspace_id {
        let write_to_disk = params.write_to_disk.unwrap_or(true);
        state::create_session(ws_id, write_to_disk).map_err(extism_pdk::Error::msg)?;
    }

    host_bridge::log_message("info", "Sync plugin initialized");
    Ok(String::new())
}

/// Shut down the plugin (persist state).
#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    if let Err(e) = state::shutdown_state() {
        host_bridge::log_message("warn", &format!("Shutdown state cleanup failed: {e}"));
    }
    host_bridge::log_message("info", "Sync plugin shut down");
    Ok(String::new())
}

/// Handle a structured command.
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;

    let result = match state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
            &s.sync_plugin,
            &req.command,
            req.params,
        ))
    }) {
        Ok(result) => result,
        Err(e) => {
            let response = CommandResponse {
                success: false,
                data: None,
                error: Some(e.to_string()),
            };
            return Ok(serde_json::to_string(&response)?);
        }
    };

    let response = match result {
        Some(Ok(data)) => CommandResponse {
            success: true,
            data: Some(data),
            error: None,
        },
        Some(Err(e)) => CommandResponse {
            success: false,
            data: None,
            error: Some(e.to_string()),
        },
        None => CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Unknown command: {}", req.command)),
        },
    };

    Ok(serde_json::to_string(&response)?)
}

/// Handle a filesystem/workspace event from the host.
#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: GuestEvent = serde_json::from_str(&input)?;

    match event.event_type.as_str() {
        "file_saved" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                // Forward file save to sync plugin - update CRDT metadata
                if let Err(e) = state::with_state(|s| {
                    // Read the file content and update body CRDT
                    if let Ok(content) = host_bridge::read_file(path) {
                        let body_docs = s.sync_plugin.body_docs();
                        let doc = body_docs.get_or_create(path);
                        let _ = doc.set_body(&content);
                    }
                }) {
                    host_bridge::log_message("warn", &format!("[on_event:file_saved] {e}"));
                }
            }
        }
        "file_created" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                if let Err(e) = state::with_state(|s| {
                    if let Ok(content) = host_bridge::read_file(path) {
                        let body_docs = s.sync_plugin.body_docs();
                        let doc = body_docs.get_or_create(path);
                        let _ = doc.set_body(&content);
                    }
                }) {
                    host_bridge::log_message("warn", &format!("[on_event:file_created] {e}"));
                }
            }
        }
        "file_deleted" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                if let Err(e) = state::with_state(|s| {
                    let body_docs = s.sync_plugin.body_docs();
                    let _ = body_docs.delete(path);
                }) {
                    host_bridge::log_message("warn", &format!("[on_event:file_deleted] {e}"));
                }
            }
        }
        "file_renamed" | "file_moved" => {
            let old_path = event.payload.get("old_path").and_then(|v| v.as_str());
            let new_path = event.payload.get("new_path").and_then(|v| v.as_str());
            if let (Some(old), Some(new)) = (old_path, new_path) {
                if let Err(e) = state::with_state(|s| {
                    let body_docs = s.sync_plugin.body_docs();
                    let _ = body_docs.rename(old, new);
                }) {
                    host_bridge::log_message("warn", &format!("[on_event:file_renamed] {e}"));
                }
            }
        }
        "workspace_opened" => {
            if let Some(root) = event.payload.get("workspace_root").and_then(|v| v.as_str()) {
                if let Err(e) = state::with_state(|s| {
                    let event = diaryx_core::plugin::WorkspaceOpenedEvent {
                        workspace_root: std::path::PathBuf::from(root),
                    };
                    poll_future(diaryx_core::plugin::WorkspacePlugin::on_workspace_opened(
                        &s.sync_plugin,
                        &event,
                    ));
                }) {
                    host_bridge::log_message("warn", &format!("[on_event:workspace_opened] {e}"));
                }
            }
        }
        other => {
            host_bridge::log_message("debug", &format!("Unhandled event type: {other}"));
        }
    }

    Ok(String::new())
}

/// Get plugin configuration.
#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = match state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::get_config(
            &s.sync_plugin,
        ))
    }) {
        Ok(config) => config,
        Err(e) => {
            host_bridge::log_message("warn", &format!("[get_config] {e}"));
            None
        }
    };
    match config {
        Some(val) => Ok(serde_json::to_string(&val)?),
        None => Ok("{}".into()),
    }
}

/// Set plugin configuration.
#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: JsonValue = serde_json::from_str(&input)?;
    if let Err(e) = state::with_state(|s| {
        let _ = poll_future(diaryx_core::plugin::WorkspacePlugin::set_config(
            &s.sync_plugin,
            config,
        ));
    }) {
        host_bridge::log_message("warn", &format!("[set_config] {e}"));
    }
    Ok(String::new())
}

// ============================================================================
// Binary exports (hot path)
// ============================================================================

/// Handle an incoming binary WebSocket message.
/// Input: raw framed v2 sync message bytes.
/// Output: binary action envelope (see binary_protocol module).
#[plugin_fn]
pub fn handle_binary_message(input: Vec<u8>) -> FnResult<Vec<u8>> {
    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::BinaryMessage(input)))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[handle_binary_message] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

/// Handle an incoming text WebSocket message (control/handshake).
/// Input: JSON text message.
/// Output: binary action envelope.
#[plugin_fn]
pub fn handle_text_message(input: String) -> FnResult<Vec<u8>> {
    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::TextMessage(input)))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[handle_text_message] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

/// Called when a WebSocket connection is established.
/// Input: connection info JSON (workspace_id, etc.)
/// Output: binary action envelope with initial sync messages.
#[plugin_fn]
pub fn on_connected(input: String) -> FnResult<Vec<u8>> {
    // Parse connection params if provided
    if let Ok(params) = serde_json::from_str::<InitParams>(&input) {
        if let Some(ws_id) = params.workspace_id {
            let write_to_disk = params.write_to_disk.unwrap_or(true);
            if let Err(e) = state::create_session(&ws_id, write_to_disk) {
                host_bridge::log_message(
                    "warn",
                    &format!("[on_connected] create_session failed: {e}"),
                );
            }
        }
    }

    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::Connected))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[on_connected] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

/// Called when the WebSocket disconnects.
/// Output: binary action envelope (typically just EmitEvent(Disconnected)).
#[plugin_fn]
pub fn on_disconnected(_input: String) -> FnResult<Vec<u8>> {
    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::Disconnected))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[on_disconnected] {e}"));
        vec![]
    });

    // Persist state on disconnect
    if let Err(e) = state::persist_state() {
        host_bridge::log_message(
            "warn",
            &format!("[on_disconnected] persist_state failed: {e}"),
        );
    }

    Ok(binary_protocol::encode_actions(&actions))
}

/// Queue a local CRDT update to be sent to the server.
/// Input: JSON `{"doc_id": "...", "data": "base64..."}`.
/// Output: binary action envelope with sync messages to send.
#[plugin_fn]
pub fn queue_local_update(input: String) -> FnResult<Vec<u8>> {
    let params: JsonValue = serde_json::from_str(&input)?;
    let doc_id = params
        .get("doc_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let data_b64 = params.get("data").and_then(|v| v.as_str()).unwrap_or("");

    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .unwrap_or_default();

    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::LocalUpdate { doc_id, data }))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[queue_local_update] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

/// Called after a snapshot has been imported.
/// Output: binary action envelope.
#[plugin_fn]
pub fn on_snapshot_imported(_input: String) -> FnResult<Vec<u8>> {
    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::SnapshotImported))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[on_snapshot_imported] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

/// Request body sync for specific files.
/// Input: JSON `{"file_paths": ["path1", "path2"]}`.
/// Output: binary action envelope.
#[plugin_fn]
pub fn sync_body_files(input: String) -> FnResult<Vec<u8>> {
    let params: JsonValue = serde_json::from_str(&input)?;
    let file_paths: Vec<String> = params
        .get("file_paths")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let actions = state::try_with_state_mut(|s| {
        if let Some(session) = &s.session {
            poll_future(session.process(IncomingEvent::SyncBodyFiles { file_paths }))
        } else {
            vec![]
        }
    })
    .unwrap_or_else(|e| {
        host_bridge::log_message("warn", &format!("[sync_body_files] {e}"));
        vec![]
    });
    Ok(binary_protocol::encode_actions(&actions))
}

// ============================================================================
// Helpers
// ============================================================================

/// Execute a typed Command (same format as Diaryx::execute).
///
/// Takes a full serialized Command JSON, calls handle_typed_command on the
/// inner SyncPlugin, and returns a full serialized Response JSON.
/// Returns empty string if the command is not handled by this plugin.
#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    use diaryx_core::command::Command;

    let cmd: Command = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid command: {e}")))?;

    let result = state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_typed_command(
            &s.sync_plugin,
            &cmd,
        ))
    })
    .map_err(|e| extism_pdk::Error::msg(e))?;

    match result {
        Some(Ok(response)) => {
            let json = serde_json::to_string(&response)
                .map_err(|e| extism_pdk::Error::msg(format!("Serialize error: {e}")))?;
            Ok(json)
        }
        Some(Err(e)) => Err(extism_pdk::Error::msg(format!("{e}")).into()),
        None => Ok(String::new()),
    }
}

/// List all commands this plugin handles.
fn all_commands() -> Vec<String> {
    vec![
        // Workspace CRDT State
        "GetSyncState",
        "GetFullState",
        "ApplyRemoteUpdate",
        "GetMissingUpdates",
        "SaveCrdtState",
        // File Metadata
        "GetCrdtFile",
        "SetCrdtFile",
        "ListCrdtFiles",
        // Body Documents
        "GetBodyContent",
        "SetBodyContent",
        "ResetBodyDoc",
        "GetBodySyncState",
        "GetBodyFullState",
        "ApplyBodyUpdate",
        "GetBodyMissingUpdates",
        "SaveBodyDoc",
        "SaveAllBodyDocs",
        "ListLoadedBodyDocs",
        "UnloadBodyDoc",
        // Y-Sync Protocol
        "CreateSyncStep1",
        "HandleSyncMessage",
        "CreateUpdateMessage",
        // Sync Handler
        "ConfigureSyncHandler",
        "GetStoragePath",
        "GetCanonicalPath",
        "ApplyRemoteWorkspaceUpdateWithEffects",
        "ApplyRemoteBodyUpdateWithEffects",
        // Sync Manager
        "HandleWorkspaceSyncMessage",
        "HandleCrdtState",
        "CreateWorkspaceSyncStep1",
        "CreateWorkspaceUpdate",
        "InitBodySync",
        "CloseBodySync",
        "HandleBodySyncMessage",
        "CreateBodySyncStep1",
        "CreateBodyUpdate",
        "IsSyncComplete",
        "IsWorkspaceSynced",
        "IsBodySynced",
        "MarkSyncComplete",
        "GetActiveSyncs",
        "TrackContent",
        "IsEcho",
        "ClearTrackedContent",
        "ResetSyncState",
        "TriggerWorkspaceSync",
        // History
        "GetHistory",
        "GetFileHistory",
        "RestoreVersion",
        "GetVersionDiff",
        "GetStateAt",
        // Workspace Initialization
        "InitializeWorkspaceCrdt",
        // Status
        "get_sync_status",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Simple single-poll executor for immediately-ready futures.
///
/// In the Extism guest (single-threaded WASM), all async operations complete
/// synchronously because host function calls are synchronous. This function
/// polls the future once and returns the result.
fn poll_future<F: std::future::Future>(f: F) -> F::Output {
    use std::pin::pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );

    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);
    let mut pinned = pin!(f);

    match pinned.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("Future was not immediately ready in Extism guest"),
    }
}
