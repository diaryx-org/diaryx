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

use diaryx_core::plugin::{ComponentRef, SidebarSide, StatusBarPosition, UiContribution};
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
    #[serde(default)]
    cli: Vec<JsonValue>,
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
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct SyncExtismConfig {
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    active_join_code: Option<String>,
    #[serde(default)]
    share_read_only: Option<bool>,
}

fn normalize_server_base(server_url: &str) -> String {
    server_url
        .trim_end_matches("/sync2")
        .trim_end_matches("/sync")
        .trim_end_matches('/')
        .to_string()
}

fn load_extism_config() -> SyncExtismConfig {
    match host_bridge::storage_get("sync.extism.config") {
        Ok(Some(bytes)) => serde_json::from_slice::<SyncExtismConfig>(&bytes).unwrap_or_default(),
        _ => SyncExtismConfig::default(),
    }
}

fn save_extism_config(config: &SyncExtismConfig) {
    if let Ok(bytes) = serde_json::to_vec(config) {
        let _ = host_bridge::storage_set("sync.extism.config", &bytes);
    }
}

fn command_param_str(params: &JsonValue, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn command_param_bool(params: &JsonValue, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

fn resolve_server_url(params: &JsonValue, config: &SyncExtismConfig) -> Option<String> {
    command_param_str(params, "server_url")
        .or_else(|| config.server_url.clone())
        .map(|s| normalize_server_base(&s))
}

fn resolve_auth_token(params: &JsonValue, config: &SyncExtismConfig) -> Option<String> {
    command_param_str(params, "auth_token").or_else(|| config.auth_token.clone())
}

fn http_error(status: u64, body: &str) -> String {
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body}")
    }
}

fn sync_status_from_state() -> JsonValue {
    let (state, label) = match state::with_state(|s| s.session.is_some()) {
        Ok(true) => ("syncing", "Syncing"),
        Ok(false) | Err(_) => ("idle", "Idle"),
    };
    serde_json::json!({
        "state": state,
        "label": label,
        "detail": JsonValue::Null,
        "progress": JsonValue::Null
    })
}

fn get_component_html_by_id(component_id: &str) -> Option<&'static str> {
    match component_id {
        "sync.settings" => Some(include_str!("ui/settings.html")),
        "sync.share" => Some(include_str!("ui/share.html")),
        "sync.snapshots" => Some(include_str!("ui/snapshots.html")),
        "sync.history" => Some(include_str!("ui/history.html")),
        _ => None,
    }
}

fn auth_headers(auth_token: Option<String>) -> Vec<(String, String)> {
    match auth_token {
        Some(token) if !token.trim().is_empty() => vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), format!("Bearer {}", token)),
        ],
        _ => vec![("Content-Type".to_string(), "application/json".to_string())],
    }
}

fn parse_http_status(response: &JsonValue) -> u64 {
    response.get("status").and_then(|v| v.as_u64()).unwrap_or(0)
}

fn parse_http_body(response: &JsonValue) -> String {
    response
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn parse_http_body_json(response: &JsonValue) -> Option<JsonValue> {
    let body = parse_http_body(response);
    if body.is_empty() {
        return None;
    }
    serde_json::from_str(&body).ok()
}

fn provider_supported(params: &JsonValue) -> bool {
    command_param_str(params, "provider_id")
        .map(|id| id == "sync")
        .unwrap_or(true)
}

fn handle_get_provider_status(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Ok(serde_json::json!({
            "ready": false,
            "message": "Unsupported provider"
        }));
    }

    let config = load_extism_config();
    let has_server = resolve_server_url(params, &config)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_auth = resolve_auth_token(params, &config)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    if !has_server {
        return Ok(serde_json::json!({
            "ready": false,
            "message": "Sync server URL is not configured"
        }));
    }
    if !has_auth {
        return Ok(serde_json::json!({
            "ready": false,
            "message": "Sign in to enable sync"
        }));
    }

    Ok(serde_json::json!({
        "ready": true,
        "message": JsonValue::Null
    }))
}

fn handle_list_remote_workspaces(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Ok(serde_json::json!({ "workspaces": Vec::<JsonValue>::new() }));
    }
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let response =
        host_bridge::http_request("GET", &format!("{server}/api/workspaces"), &headers, None)?;
    let status = parse_http_status(&response);
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    let body = parse_http_body_json(&response).unwrap_or(JsonValue::Array(Vec::new()));
    let workspaces = body
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id")?.as_str()?.to_string();
            let name = value.get("name")?.as_str()?.to_string();
            Some(serde_json::json!({ "id": id, "name": name }))
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "workspaces": workspaces }))
}

fn handle_link_workspace(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Err("Unsupported provider".to_string());
    }
    let mut remote_id = command_param_str(params, "remote_id");
    let mut created_remote = false;
    let snapshot_uploaded = false;

    if remote_id.is_none() {
        let config = load_extism_config();
        let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
        let headers = auth_headers(resolve_auth_token(params, &config));
        let name = command_param_str(params, "name").ok_or("Missing name")?;
        let response = host_bridge::http_request(
            "POST",
            &format!("{server}/api/workspaces"),
            &headers,
            Some(serde_json::json!({ "name": name })),
        )?;
        let status = parse_http_status(&response);
        if status != 200 {
            return Err(http_error(status, &parse_http_body(&response)));
        }
        let body = parse_http_body_json(&response).ok_or("Invalid workspace response")?;
        remote_id = body
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        created_remote = true;
    }

    let remote_id = remote_id.ok_or("Missing remote_id")?;
    Ok(serde_json::json!({
        "remote_id": remote_id,
        "created_remote": created_remote,
        "snapshot_uploaded": snapshot_uploaded
    }))
}

fn handle_unlink_workspace(_params: &JsonValue) -> JsonValue {
    serde_json::json!({ "ok": true })
}

fn handle_download_workspace(_params: &JsonValue) -> JsonValue {
    serde_json::json!({ "files_imported": 0 })
}

fn handle_create_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let workspace_id = command_param_str(params, "workspace_id").ok_or("Missing workspace_id")?;
    let read_only = command_param_bool(params, "read_only").unwrap_or(false);

    let response = host_bridge::http_request(
        "POST",
        &format!("{server}/api/sessions"),
        &headers,
        Some(serde_json::json!({
            "workspace_id": workspace_id,
            "read_only": read_only
        })),
    )?;
    let status = parse_http_status(&response);
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }

    let body = parse_http_body_json(&response).ok_or("Invalid session response")?;
    let join_code = body
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or("Missing code in session response")?
        .to_string();
    let workspace_id = body
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let read_only = body
        .get("read_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut updated = config;
    updated.active_join_code = Some(join_code.clone());
    updated.share_read_only = Some(read_only);
    if !workspace_id.is_empty() {
        updated.workspace_id = Some(workspace_id.clone());
    }
    save_extism_config(&updated);

    Ok(serde_json::json!({
        "join_code": join_code,
        "workspace_id": workspace_id,
        "read_only": read_only
    }))
}

fn handle_join_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let join_code = command_param_str(params, "join_code").ok_or("Missing join_code")?;

    let response = host_bridge::http_request(
        "GET",
        &format!("{server}/api/sessions/{}", join_code.to_uppercase()),
        &[("Content-Type".to_string(), "application/json".to_string())],
        None,
    )?;
    let status = parse_http_status(&response);
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }

    let body = parse_http_body_json(&response).ok_or("Invalid session response")?;
    let workspace_id = body
        .get("workspace_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let read_only = body
        .get("read_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut updated = config;
    updated.active_join_code = Some(join_code.to_uppercase());
    updated.share_read_only = Some(read_only);
    if !workspace_id.is_empty() {
        updated.workspace_id = Some(workspace_id.clone());
    }
    save_extism_config(&updated);

    Ok(serde_json::json!({
        "workspace_id": workspace_id,
        "read_only": read_only
    }))
}

fn handle_end_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let mut config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let join_code =
        command_param_str(params, "join_code").or_else(|| config.active_join_code.clone());

    if let Some(join_code) = join_code {
        let response = host_bridge::http_request(
            "DELETE",
            &format!("{server}/api/sessions/{}", join_code.to_uppercase()),
            &headers,
            None,
        )?;
        let status = parse_http_status(&response);
        if status != 204 && status != 200 {
            return Err(http_error(status, &parse_http_body(&response)));
        }
    }

    config.active_join_code = None;
    save_extism_config(&config);
    Ok(serde_json::json!({ "ok": true }))
}

fn handle_set_share_read_only(params: &JsonValue) -> Result<JsonValue, String> {
    let mut config = load_extism_config();
    let read_only = command_param_bool(params, "read_only").unwrap_or(false);

    let join_code = config.active_join_code.clone();
    if let Some(code) = join_code {
        let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
        let headers = auth_headers(resolve_auth_token(params, &config));
        let response = host_bridge::http_request(
            "PATCH",
            &format!("{server}/api/sessions/{}", code.to_uppercase()),
            &headers,
            Some(serde_json::json!({ "read_only": read_only })),
        )?;
        let status = parse_http_status(&response);
        if status != 200 {
            return Err(http_error(status, &parse_http_body(&response)));
        }
    }

    config.share_read_only = Some(read_only);
    save_extism_config(&config);
    Ok(serde_json::json!({ "read_only": read_only }))
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
        fields: vec![],
        component: Some(ComponentRef::Iframe {
            component_id: "sync.settings".into(),
        }),
    };

    let share_tab = UiContribution::SidebarTab {
        id: "share".into(),
        label: "Share".into(),
        icon: Some("share".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Iframe {
            component_id: "sync.share".into(),
        },
    };

    let snapshots_tab = UiContribution::SidebarTab {
        id: "snapshots".into(),
        label: "Snapshots".into(),
        icon: Some("history".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Iframe {
            component_id: "sync.snapshots".into(),
        },
    };

    let history_tab = UiContribution::SidebarTab {
        id: "history".into(),
        label: "History".into(),
        icon: Some("history".into()),
        side: SidebarSide::Right,
        component: ComponentRef::Iframe {
            component_id: "sync.history".into(),
        },
    };

    let status_bar_item = UiContribution::StatusBarItem {
        id: "sync-status".into(),
        label: "Sync".into(),
        position: StatusBarPosition::Right,
        plugin_command: Some("GetSyncStatus".into()),
    };

    let workspace_provider = UiContribution::WorkspaceProvider {
        id: "diaryx.sync".into(),
        label: "Diaryx Sync".into(),
        icon: Some("cloud".into()),
    };

    let manifest = GuestManifest {
        id: "diaryx.sync".into(),
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
            serde_json::to_value(&workspace_provider).unwrap_or_default(),
        ],
        commands: all_commands(),
        cli: vec![serde_json::json!({
            "name": "sync",
            "about": "Sync workspace with remote server",
            "aliases": ["sy"],
            "subcommands": [
                {
                    "name": "login", "about": "Authenticate via magic link",
                    "native_handler": "sync_login", "requires_workspace": false,
                    "args": [
                        {"name": "email", "required": true, "help": "Email address"},
                        {"name": "server", "short": "s", "long": "server", "help": "Server URL"}
                    ]
                },
                {
                    "name": "verify", "about": "Complete authentication",
                    "native_handler": "sync_verify", "requires_workspace": false,
                    "args": [
                        {"name": "token", "required": true, "help": "Verification token"},
                        {"name": "device-name", "long": "device-name", "help": "Device name"}
                    ]
                },
                {
                    "name": "logout", "about": "Clear credentials",
                    "native_handler": "sync_logout", "requires_workspace": false
                },
                {
                    "name": "status", "about": "Show sync status",
                    "native_handler": "sync_status"
                },
                {
                    "name": "start", "about": "Start continuous sync",
                    "native_handler": "sync_start",
                    "args": [
                        {"name": "background", "short": "b", "long": "background",
                         "is_flag": true, "help": "Run in background"}
                    ]
                },
                {
                    "name": "push", "about": "Push local changes",
                    "native_handler": "sync_push"
                },
                {
                    "name": "pull", "about": "Pull remote changes",
                    "native_handler": "sync_pull"
                },
                {
                    "name": "config", "about": "Configure sync settings",
                    "native_handler": "sync_config",
                    "args": [
                        {"name": "server", "long": "server", "help": "Set server URL"},
                        {"name": "workspace-id", "long": "workspace-id", "help": "Set workspace ID"},
                        {"name": "show", "long": "show", "is_flag": true, "help": "Show current config"}
                    ]
                }
            ]
        })],
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
        server_url: None,
        auth_token: None,
    });

    let mut extism_config = load_extism_config();
    if let Some(workspace_id) = &params.workspace_id {
        extism_config.workspace_id = Some(workspace_id.clone());
    }
    if let Some(server_url) = &params.server_url {
        extism_config.server_url = Some(server_url.clone());
    }
    if let Some(auth_token) = &params.auth_token {
        extism_config.auth_token = Some(auth_token.clone());
    }
    save_extism_config(&extism_config);

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

    let custom_result: Option<Result<JsonValue, String>> = match req.command.as_str() {
        "get_component_html" => {
            let component_id = req
                .params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            Some(
                get_component_html_by_id(component_id)
                    .map(|html| JsonValue::String(html.to_string()))
                    .ok_or_else(|| format!("Unknown sync component: {component_id}")),
            )
        }
        "get_config" => Some(Ok(
            serde_json::to_value(load_extism_config()).unwrap_or_default()
        )),
        "set_config" => {
            let mut config = load_extism_config();
            if let Some(server_url) = req.params.get("server_url").and_then(|v| v.as_str()) {
                config.server_url = Some(server_url.to_string());
            }
            if let Some(auth_token) = req.params.get("auth_token").and_then(|v| v.as_str()) {
                config.auth_token = Some(auth_token.to_string());
            }
            if let Some(workspace_id) = req.params.get("workspace_id").and_then(|v| v.as_str()) {
                config.workspace_id = Some(workspace_id.to_string());
            }
            if let Some(join_code) = req.params.get("active_join_code").and_then(|v| v.as_str()) {
                config.active_join_code = Some(join_code.to_string());
            }
            if let Some(read_only) = req.params.get("share_read_only").and_then(|v| v.as_bool()) {
                config.share_read_only = Some(read_only);
            }
            save_extism_config(&config);
            Some(Ok(JsonValue::Null))
        }
        "GetSyncStatus" => Some(Ok(sync_status_from_state())),
        "GetProviderStatus" => Some(handle_get_provider_status(&req.params)),
        "ListRemoteWorkspaces" => Some(handle_list_remote_workspaces(&req.params)),
        "LinkWorkspace" => Some(handle_link_workspace(&req.params)),
        "UnlinkWorkspace" => Some(Ok(handle_unlink_workspace(&req.params))),
        "DownloadWorkspace" => Some(Ok(handle_download_workspace(&req.params))),
        "CreateShareSession" => Some(handle_create_share_session(&req.params)),
        "JoinShareSession" => Some(handle_join_share_session(&req.params)),
        "EndShareSession" => Some(handle_end_share_session(&req.params)),
        "SetShareReadOnly" => Some(handle_set_share_read_only(&req.params)),
        _ => None,
    };

    if let Some(result) = custom_result {
        let response = match result {
            Ok(data) => CommandResponse {
                success: true,
                data: Some(data),
                error: None,
            },
            Err(error) => CommandResponse {
                success: false,
                data: None,
                error: Some(error),
            },
        };
        return Ok(serde_json::to_string(&response)?);
    }

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
    Ok(serde_json::to_string(&load_extism_config())?)
}

/// Set plugin configuration.
#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let incoming: JsonValue = serde_json::from_str(&input)?;
    let mut config = load_extism_config();
    if let Some(server_url) = incoming.get("server_url").and_then(|v| v.as_str()) {
        config.server_url = Some(server_url.to_string());
    }
    if let Some(auth_token) = incoming.get("auth_token").and_then(|v| v.as_str()) {
        config.auth_token = Some(auth_token.to_string());
    }
    if let Some(workspace_id) = incoming.get("workspace_id").and_then(|v| v.as_str()) {
        config.workspace_id = Some(workspace_id.to_string());
    }
    if let Some(join_code) = incoming.get("active_join_code").and_then(|v| v.as_str()) {
        config.active_join_code = Some(join_code.to_string());
    }
    if let Some(read_only) = incoming.get("share_read_only").and_then(|v| v.as_bool()) {
        config.share_read_only = Some(read_only);
    }
    save_extism_config(&config);
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
/// Takes a JSON object with `type` and optional `params` fields, extracts
/// them, and calls `handle_command` on the inner SyncPlugin.
/// Returns the result as a serialized JSON string.
/// Returns empty string if the command is not handled by this plugin.
#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    let parsed: serde_json::Value = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid JSON: {e}")))?;

    // Extract command type and params from the tagged enum format
    // Commands are serialized as { "type": "CommandName", "params": { ... } }
    let cmd_type = parsed["type"]
        .as_str()
        .ok_or_else(|| extism_pdk::Error::msg("Missing 'type' field in command"))?;

    let params = parsed
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let result = state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
            &s.sync_plugin,
            cmd_type,
            params,
        ))
    })
    .map_err(|e| extism_pdk::Error::msg(e))?;

    match result {
        Some(Ok(value)) => {
            // Wrap as PluginResult for consistency with the Response enum
            let response = serde_json::json!({ "type": "PluginResult", "data": value });
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
        "GetSyncStatus",
        // Workspace Provider
        "GetProviderStatus",
        "ListRemoteWorkspaces",
        "LinkWorkspace",
        "UnlinkWorkspace",
        "DownloadWorkspace",
        // Share Session
        "CreateShareSession",
        "JoinShareSession",
        "EndShareSession",
        "SetShareReadOnly",
        // Iframe Components
        "get_component_html",
        "get_config",
        "set_config",
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
