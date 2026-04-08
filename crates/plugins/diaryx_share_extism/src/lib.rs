//! Extism guest plugin for Diaryx live sharing.
//!
//! `diaryx.share` owns live collaborative sessions using CRDT via Durable Objects.
//! Share sessions are ephemeral — only the file manifest and actively opened files
//! are synced, not the entire workspace.

#[cfg(not(target_arch = "wasm32"))]
mod native_extism_stubs;

mod file_doc;
mod manifest;
mod session;
mod wire;

use diaryx_plugin_sdk::prelude::*;

use diaryx_core::plugin::{ComponentRef, SettingsField, SidebarSide, UiContribution};
use diaryx_plugin_sdk::protocol::ServerFunctionDecl;
use extism_pdk::*;
use serde_json::Value as JsonValue;

use session::ShareRole;

// ============================================================================
// HTTP compat helpers
// ============================================================================

fn http_request_compat(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body_json: Option<JsonValue>,
) -> Result<JsonValue, String> {
    let header_map: std::collections::HashMap<String, String> = headers.iter().cloned().collect();
    let body_str = body_json.map(|b| b.to_string());
    let resp = host::http::request(method, url, &header_map, body_str.as_deref())?;
    let mut result = serde_json::json!({
        "status": resp.status,
        "body": resp.body,
    });
    if let Some(b64) = &resp.body_base64 {
        result["body_base64"] = JsonValue::String(b64.clone());
    }
    Ok(result)
}

// ============================================================================
// Config & runtime context
// ============================================================================

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
struct ShareExtismConfig {
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    active_join_code: Option<String>,
    #[serde(default)]
    share_read_only: Option<bool>,
    #[serde(default)]
    share_role: Option<String>,
    #[serde(default)]
    session_namespace_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct RuntimeContext {
    #[serde(skip)]
    raw: JsonValue,
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    guest_mode: bool,
    #[serde(default)]
    current_workspace: Option<RuntimeWorkspace>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize, Default)]
struct RuntimeWorkspace {
    #[serde(default)]
    local_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl RuntimeContext {
    fn has_field(&self, key: &str) -> bool {
        self.raw.get(key).is_some()
    }
}

fn load_extism_config() -> ShareExtismConfig {
    match host::storage::get("share.extism.config") {
        Ok(Some(bytes)) => serde_json::from_slice::<ShareExtismConfig>(&bytes).unwrap_or_default(),
        _ => ShareExtismConfig::default(),
    }
}

fn save_extism_config(config: &ShareExtismConfig) {
    if let Ok(bytes) = serde_json::to_vec(config) {
        let _ = host::storage::set("share.extism.config", &bytes);
    }
}

fn normalize_server_base(server_url: &str) -> String {
    let mut base = server_url.trim().trim_end_matches('/').to_string();
    loop {
        if let Some(stripped) = base.strip_suffix("/sync2") {
            base = stripped.trim_end_matches('/').to_string();
            continue;
        }
        if let Some(stripped) = base.strip_suffix("/sync") {
            base = stripped.trim_end_matches('/').to_string();
            continue;
        }
        break;
    }
    base
}

fn command_param_str(params: &JsonValue, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn command_param_bool(params: &JsonValue, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

fn apply_config_patch(config: &mut ShareExtismConfig, incoming: &JsonValue) {
    apply_config_string(config, incoming, "server_url", |cfg, value| {
        cfg.server_url = value
    });
    apply_config_string(config, incoming, "auth_token", |cfg, value| {
        cfg.auth_token = value
    });
    apply_config_string(config, incoming, "active_join_code", |cfg, value| {
        cfg.active_join_code = value.map(|s| s.to_uppercase())
    });
    apply_config_bool(config, incoming, "share_read_only", |cfg, value| {
        cfg.share_read_only = value
    });
    apply_config_string(config, incoming, "share_role", |cfg, value| {
        cfg.share_role = value
    });
    apply_config_string(config, incoming, "session_namespace_id", |cfg, value| {
        cfg.session_namespace_id = value
    });
}

fn apply_config_string<F>(config: &mut ShareExtismConfig, incoming: &JsonValue, key: &str, set: F)
where
    F: FnOnce(&mut ShareExtismConfig, Option<String>),
{
    if let Some(raw) = incoming.get(key) {
        if raw.is_null() {
            set(config, None);
        } else if let Some(value) = raw.as_str() {
            let normalized = value.trim();
            if normalized.is_empty() {
                set(config, None);
            } else {
                set(config, Some(normalized.to_string()));
            }
        }
    }
}

fn apply_config_bool<F>(config: &mut ShareExtismConfig, incoming: &JsonValue, key: &str, set: F)
where
    F: FnOnce(&mut ShareExtismConfig, Option<bool>),
{
    if let Some(raw) = incoming.get(key) {
        if raw.is_null() {
            set(config, None);
        } else if let Some(value) = raw.as_bool() {
            set(config, Some(value));
        }
    }
}

fn load_runtime_context() -> RuntimeContext {
    let raw = host::context::get().unwrap_or_else(|_| serde_json::json!({}));
    let mut runtime = serde_json::from_value::<RuntimeContext>(raw.clone()).unwrap_or_default();
    runtime.raw = raw;
    runtime
}

fn trim_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_server_url(
    params: &JsonValue,
    config: &ShareExtismConfig,
    runtime: &RuntimeContext,
) -> Result<String, String> {
    trim_option(command_param_str(params, "server_url"))
        .or_else(|| {
            runtime
                .has_field("server_url")
                .then(|| trim_option(runtime.server_url.clone()))
                .flatten()
        })
        .or_else(|| trim_option(config.server_url.clone()))
        .map(|value| normalize_server_base(&value))
        .ok_or("Missing server_url".to_string())
}

fn resolve_auth_token(
    params: &JsonValue,
    config: &ShareExtismConfig,
    runtime: &RuntimeContext,
) -> Option<String> {
    trim_option(command_param_str(params, "auth_token"))
        .or_else(|| {
            runtime
                .has_field("auth_token")
                .then(|| trim_option(runtime.auth_token.clone()))
                .flatten()
        })
        .or_else(|| trim_option(config.auth_token.clone()))
}

fn runtime_workspace_root(params: &JsonValue, runtime: &RuntimeContext) -> Option<String> {
    command_param_str(params, "workspace_root").or_else(|| {
        runtime
            .current_workspace
            .as_ref()
            .and_then(|w| w.path.clone())
    })
}

fn current_workspace_root() -> Option<String> {
    load_runtime_context()
        .current_workspace
        .and_then(|workspace| workspace.path)
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

fn http_error(status: u64, body: &str) -> String {
    if body.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body}")
    }
}

fn effective_config(config: &ShareExtismConfig, runtime: &RuntimeContext) -> ShareExtismConfig {
    ShareExtismConfig {
        server_url: resolve_server_url(&JsonValue::Null, config, runtime).ok(),
        auth_token: None,
        active_join_code: config.active_join_code.clone(),
        share_read_only: config.share_read_only,
        share_role: config.share_role.clone(),
        session_namespace_id: config.session_namespace_id.clone(),
    }
}

// ============================================================================
// Session REST API
// ============================================================================

fn lookup_share_session(server: &str, join_code: &str) -> Result<(String, bool), String> {
    let response = http_request_compat(
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
    let namespace_id = body
        .get("namespace_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing namespace_id in session response")?
        .to_string();
    let read_only = body
        .get("read_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok((namespace_id, read_only))
}

fn create_share_session_api(
    server: &str,
    auth_token: Option<String>,
    namespace_id: &str,
    read_only: bool,
) -> Result<(String, bool), String> {
    let response = http_request_compat(
        "POST",
        &format!("{server}/api/sessions"),
        &auth_headers(auth_token),
        Some(serde_json::json!({
            "namespace_id": namespace_id,
            "read_only": read_only,
        })),
    )?;
    let status = parse_http_status(&response);
    if status != 200 && status != 201 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    let body = parse_http_body_json(&response).ok_or("Invalid session response")?;
    let join_code = body
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or("Missing code in session response")?
        .to_string();
    let read_only = body
        .get("read_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok((join_code, read_only))
}

fn delete_share_session_api(
    server: &str,
    auth_token: Option<String>,
    join_code: &str,
) -> Result<(), String> {
    let response = http_request_compat(
        "DELETE",
        &format!("{server}/api/sessions/{}", join_code.to_uppercase()),
        &auth_headers(auth_token),
        None,
    )?;
    let status = parse_http_status(&response);
    if status == 200 || status == 204 {
        Ok(())
    } else {
        Err(http_error(status, &parse_http_body(&response)))
    }
}

fn update_share_read_only_api(
    server: &str,
    auth_token: Option<String>,
    join_code: &str,
    read_only: bool,
) -> Result<(), String> {
    let response = http_request_compat(
        "PATCH",
        &format!("{server}/api/sessions/{}", join_code.to_uppercase()),
        &auth_headers(auth_token),
        Some(serde_json::json!({ "read_only": read_only })),
    )?;
    let status = parse_http_status(&response);
    if status == 200 {
        Ok(())
    } else {
        Err(http_error(status, &parse_http_body(&response)))
    }
}

// ============================================================================
// Namespace helpers
// ============================================================================

fn ensure_namespace(
    server: &str,
    auth_token: Option<String>,
    config: &mut ShareExtismConfig,
) -> Result<String, String> {
    if let Some(ns_id) = config.session_namespace_id.clone() {
        return Ok(ns_id);
    }

    let response = http_request_compat(
        "POST",
        &format!("{server}/api/namespaces"),
        &auth_headers(auth_token),
        Some(serde_json::json!({})),
    )?;
    let status = parse_http_status(&response);
    if status != 200 && status != 201 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    let body = parse_http_body_json(&response).ok_or("Invalid namespace response")?;
    let ns_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing id in namespace response")?
        .to_string();

    config.session_namespace_id = Some(ns_id.clone());
    save_extism_config(config);
    Ok(ns_id)
}

// ============================================================================
// Command handlers
// ============================================================================

fn handle_create_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let runtime = load_runtime_context();
    let mut config = load_extism_config();
    let server = resolve_server_url(params, &config, &runtime)?;
    let auth_token = resolve_auth_token(params, &config, &runtime);
    let read_only = command_param_bool(params, "read_only")
        .or(config.share_read_only)
        .unwrap_or(false);

    let namespace_id = ensure_namespace(&server, auth_token.clone(), &mut config)?;
    let (join_code, effective_read_only) =
        create_share_session_api(&server, auth_token.clone(), &namespace_id, read_only)?;

    // Initialize session state
    session::init_session(
        ShareRole::Host,
        namespace_id.clone(),
        join_code.clone(),
        false,
    );

    // Build manifest from workspace and set it on the session
    let workspace_root = runtime_workspace_root(params, &runtime);
    if let Some(root) = workspace_root.as_deref() {
        let manifest_doc = manifest::build_manifest_from_workspace(root);
        session::with_session_mut(|s| {
            s.manifest_doc = manifest_doc;
        });
    }

    // Connect WebSocket
    let ws_url = format!("{server}/api/sync/{namespace_id}");
    host::ws::connect(
        &ws_url,
        &namespace_id,
        auth_token.as_deref(),
        None,
        Some(true),
    )?;

    // Send manifest state
    let manifest_state = session::with_session(|s| manifest::encode_full_state(&s.manifest_doc));
    if let Some(state) = manifest_state {
        let doc_id = wire::manifest_doc_id(&namespace_id);
        let frame = wire::frame_binary(&doc_id, &state);
        host::ws::send_binary(&frame)?;
    }

    config.active_join_code = Some(join_code.clone());
    config.share_read_only = Some(effective_read_only);
    config.share_role = Some("host".to_string());
    config.session_namespace_id = Some(namespace_id.clone());
    save_extism_config(&config);

    Ok(serde_json::json!({
        "join_code": join_code,
        "namespace_id": namespace_id,
        "read_only": effective_read_only,
        "message": format!("Session created. Join code: {join_code}"),
        "config_patch": {
            "active_join_code": join_code,
            "share_read_only": effective_read_only,
            "share_role": "host",
            "session_namespace_id": namespace_id,
        }
    }))
}

fn handle_join_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let runtime = load_runtime_context();
    let config = load_extism_config();
    let server = resolve_server_url(params, &config, &runtime)?;

    let join_code = command_param_str(params, "join_code")
        .or(config.active_join_code.clone())
        .ok_or("Missing join_code")?
        .to_uppercase();

    let (namespace_id, read_only) = lookup_share_session(&server, &join_code)?;

    Ok(serde_json::json!({
        "join_code": join_code,
        "namespace_id": namespace_id,
        "read_only": read_only,
        "host_action": "enter-guest-workspace",
        "session_code": join_code,
    }))
}

fn handle_finalize_join_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let runtime = load_runtime_context();
    let mut config = load_extism_config();
    let server = resolve_server_url(params, &config, &runtime)?;
    let auth_token = resolve_auth_token(params, &config, &runtime);

    let join_code = command_param_str(params, "join_code")
        .or(config.active_join_code.clone())
        .ok_or("Missing join_code")?
        .to_uppercase();

    let (namespace_id, read_only) = lookup_share_session(&server, &join_code)?;

    // Initialize session state as guest
    session::init_session(
        ShareRole::Guest,
        namespace_id.clone(),
        join_code.clone(),
        read_only,
    );

    // Connect WebSocket with session code for guest auth
    let ws_url = format!("{server}/api/sync/{namespace_id}?session={join_code}");
    host::ws::connect(
        &ws_url,
        &namespace_id,
        auth_token.as_deref(),
        Some(&join_code),
        Some(!read_only),
    )?;

    config.active_join_code = Some(join_code.clone());
    config.share_read_only = Some(read_only);
    config.share_role = Some("guest".to_string());
    config.session_namespace_id = Some(namespace_id);
    save_extism_config(&config);

    Ok(serde_json::json!({
        "join_code": join_code,
        "read_only": read_only,
    }))
}

fn handle_end_share_session(params: &JsonValue) -> Result<JsonValue, String> {
    let runtime = load_runtime_context();
    let config = load_extism_config();
    let server = resolve_server_url(params, &config, &runtime)?;
    let auth_token = resolve_auth_token(params, &config, &runtime);

    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);

    // Reconcile modified files if host
    if is_host {
        if let Some(workspace_root) = current_workspace_root() {
            session::with_session(|s| {
                for (path, doc) in &s.open_files {
                    let content = file_doc::read_file_doc(doc);
                    let full_path = format!("{}/{}", workspace_root.trim_end_matches('/'), path);
                    let current = host::fs::read_file(&full_path).unwrap_or_default();
                    if content != current {
                        let _ = host::fs::write_file(&full_path, &content);
                    }
                }
            });
        }
    }

    // Signal session end
    let _ = host::ws::send_text(&wire::make_session_end());
    let _ = host::ws::disconnect();

    // Delete session on server if host
    if is_host {
        if let Some(join_code) = &config.active_join_code {
            let _ = delete_share_session_api(&server, auth_token, join_code);
        }
    }

    session::destroy_session();

    // Clear config
    let mut config = load_extism_config();
    config.active_join_code = None;
    config.share_role = None;
    config.share_read_only = None;
    config.session_namespace_id = None;
    save_extism_config(&config);

    let mut result = serde_json::json!({
        "message": "Session ended",
        "config_patch": {
            "active_join_code": null,
            "share_role": null,
            "share_read_only": null,
            "session_namespace_id": null,
        }
    });
    if !is_host {
        result["host_action"] = JsonValue::String("leave-guest-workspace".to_string());
    }
    Ok(result)
}

fn handle_set_share_read_only(params: &JsonValue) -> Result<JsonValue, String> {
    let runtime = load_runtime_context();
    let config = load_extism_config();
    let server = resolve_server_url(params, &config, &runtime)?;
    let auth_token = resolve_auth_token(params, &config, &runtime);
    let read_only = command_param_bool(params, "read_only")
        .or(config.share_read_only)
        .unwrap_or(false);

    let join_code = config
        .active_join_code
        .as_ref()
        .ok_or("No active session")?;

    update_share_read_only_api(&server, auth_token, join_code, read_only)?;

    let mut config = load_extism_config();
    config.share_read_only = Some(read_only);
    save_extism_config(&config);

    Ok(serde_json::json!({
        "read_only": read_only,
        "config_patch": { "share_read_only": read_only },
    }))
}

// ============================================================================
// Manifest & command dispatch
// ============================================================================

fn build_manifest() -> GuestManifest {
    let share_settings_tab = UiContribution::SettingsTab {
        id: "share-settings".into(),
        label: "Live Share".into(),
        icon: None,
        fields: vec![
            SettingsField::AuthStatus {
                label: "Account".into(),
                description: Some("Sign in to host live sharing sessions.".into()),
            },
            SettingsField::UpgradeBanner {
                feature: "Live Share".into(),
                description: Some("Upgrade to share workspaces in real time.".into()),
            },
        ],
        component: None,
    };

    let share_tab = UiContribution::SidebarTab {
        id: "share".into(),
        label: "Share".into(),
        icon: Some("share".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Declarative {
            fields: vec![
                SettingsField::Section {
                    label: "Host Session".into(),
                    description: Some("Create a live session for your current workspace.".into()),
                },
                SettingsField::Toggle {
                    key: "share_read_only".into(),
                    label: "Read-only".into(),
                    description: Some("Guests can view but not edit when enabled.".into()),
                },
                SettingsField::Button {
                    label: "Create Session".into(),
                    command: "CreateShareSession".into(),
                    variant: None,
                },
                SettingsField::Button {
                    label: "Apply Access".into(),
                    command: "SetShareReadOnly".into(),
                    variant: Some("outline".into()),
                },
                SettingsField::Button {
                    label: "End Session".into(),
                    command: "EndShareSession".into(),
                    variant: Some("destructive".into()),
                },
                SettingsField::Section {
                    label: "Join Session".into(),
                    description: Some("Paste a join code to enter as a guest.".into()),
                },
                SettingsField::Text {
                    key: "active_join_code".into(),
                    label: "Join Code".into(),
                    description: Some(
                        "Use an existing code or copy the active session code.".into(),
                    ),
                    placeholder: Some("ABCD1234".into()),
                },
                SettingsField::Button {
                    label: "Join Session".into(),
                    command: "JoinShareSession".into(),
                    variant: Some("outline".into()),
                },
            ],
        },
    };

    GuestManifest::new(
        "diaryx.share",
        "Live Share",
        env!("CARGO_PKG_VERSION"),
        "Real-time collaborative sharing for Diaryx workspaces",
        vec![
            "workspace_events".into(),
            "file_events".into(),
            "sync_transport".into(),
            "custom_commands".into(),
        ],
    )
    .ui(vec![
        serde_json::to_value(&share_settings_tab).unwrap_or_default(),
        serde_json::to_value(&share_tab).unwrap_or_default(),
    ])
    .commands(vec![
        "CreateShareSession".into(),
        "JoinShareSession".into(),
        "EndShareSession".into(),
        "SetShareReadOnly".into(),
        "get_config".into(),
        "set_config".into(),
    ])
    .server_functions(vec![
        ServerFunctionDecl {
            name: "create_session".into(),
            method: "POST".into(),
            path: "/api/sessions".into(),
            description: "Create a live share session for a namespace".into(),
        },
        ServerFunctionDecl {
            name: "get_session".into(),
            method: "GET".into(),
            path: "/api/sessions/{code}".into(),
            description: "Look up a session by code (unauthenticated, for guests)".into(),
        },
        ServerFunctionDecl {
            name: "update_session".into(),
            method: "PATCH".into(),
            path: "/api/sessions/{code}".into(),
            description: "Update session read_only flag (owner only)".into(),
        },
        ServerFunctionDecl {
            name: "delete_session".into(),
            method: "DELETE".into(),
            path: "/api/sessions/{code}".into(),
            description: "End a session (owner only)".into(),
        },
        ServerFunctionDecl {
            name: "sync_ws".into(),
            method: "WS".into(),
            path: "/api/sync/{namespace_id}".into(),
            description: "WebSocket CRDT relay for live share sessions".into(),
        },
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "plugin_storage": { "include": ["all"], "exclude": [] },
            "http_requests": { "include": ["all"], "exclude": [] },
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] },
            "delete_files": { "include": ["all"], "exclude": [] },
        }),
        reasons: std::collections::HashMap::new(),
    })
}

fn command_response(result: Result<JsonValue, String>) -> CommandResponse {
    match result {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
    }
}

fn execute_command(req: CommandRequest) -> CommandResponse {
    let CommandRequest { command, params } = req;
    let result = match command.as_str() {
        "get_config" => {
            let config = load_extism_config();
            let runtime = load_runtime_context();
            Ok(serde_json::to_value(effective_config(&config, &runtime)).unwrap_or_default())
        }
        "set_config" => {
            let mut config = load_extism_config();
            apply_config_patch(&mut config, &params);
            save_extism_config(&config);
            Ok(JsonValue::Null)
        }
        "CreateShareSession" => handle_create_share_session(&params),
        "JoinShareSession" => handle_join_share_session(&params),
        "FinalizeJoinShareSession" => handle_finalize_join_share_session(&params),
        "EndShareSession" => handle_end_share_session(&params),
        "SetShareReadOnly" => handle_set_share_read_only(&params),
        _ => Err(format!("Unknown command: {command}")),
    };

    command_response(result)
}

// ============================================================================
// Plugin exports
// ============================================================================

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&build_manifest())?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    let mut config = load_extism_config();
    if let Some(server_url) = params.server_url {
        config.server_url = Some(server_url);
    }
    if let Some(auth_token) = params.auth_token {
        config.auth_token = Some(auth_token);
    }
    save_extism_config(&config);
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    if session::has_session() {
        // Best-effort reconciliation on shutdown
        let _ = handle_end_share_session(&JsonValue::Null);
    }
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = execute_command(req);
    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: GuestEvent = serde_json::from_str(&input)?;

    if !session::has_session() {
        return Ok(String::new());
    }

    match event.event_type.as_str() {
        "file_opened" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = make_relative_path(path);
                handle_file_opened(&relative);
            }
        }
        "file_saved" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = make_relative_path(path);
                handle_file_saved(&relative);
            }
        }
        "file_created" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = make_relative_path(path);
                handle_file_created(&relative);
            }
        }
        "file_deleted" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = make_relative_path(path);
                handle_file_deleted(&relative);
            }
        }
        "file_renamed" | "file_moved" => {
            let old = event
                .payload
                .get("old_path")
                .and_then(|v| v.as_str())
                .map(make_relative_path);
            let new = event
                .payload
                .get("new_path")
                .and_then(|v| v.as_str())
                .map(make_relative_path);
            if let (Some(old_rel), Some(new_rel)) = (old, new) {
                handle_file_renamed(&old_rel, &new_rel);
            }
        }
        _ => {}
    }

    Ok(String::new())
}

fn make_relative_path(path: &str) -> String {
    if let Some(root) = current_workspace_root() {
        let normalized_root = root.trim_end_matches('/');
        if let Some(stripped) = path.strip_prefix(normalized_root) {
            return stripped.trim_start_matches('/').to_string();
        }
    }
    path.to_string()
}

fn handle_file_opened(relative_path: &str) {
    let already_open = session::with_session(|s| {
        s.open_files.contains_key(relative_path) || s.pending_requests.contains(relative_path)
    })
    .unwrap_or(true);

    if already_open {
        return;
    }

    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);

    if is_host {
        // Host: create file doc from local file and push it
        if let Some(workspace_root) = current_workspace_root() {
            let full_path = format!("{}/{}", workspace_root.trim_end_matches('/'), relative_path);
            if let Ok(content) = host::fs::read_file(&full_path) {
                let doc = file_doc::create_file_doc(&content);
                let state = file_doc::encode_full_state(&doc);

                session::with_session_mut(|s| {
                    let doc_id = wire::file_doc_id(&s.namespace_id, relative_path);
                    let frame = wire::frame_binary(&doc_id, &state);
                    let _ = host::ws::send_binary(&frame);
                    let _ = host::ws::send_text(&wire::make_file_ready(relative_path));
                    s.open_files.insert(relative_path.to_string(), doc);
                });
            }
        }
    } else {
        // Guest: request file from host
        session::with_session_mut(|s| {
            s.pending_requests.insert(relative_path.to_string());
        });
        let _ = host::ws::send_text(&wire::make_file_request(relative_path));
    }
}

fn handle_file_saved(relative_path: &str) {
    let should_send =
        session::with_session(|s| !s.read_only && s.open_files.contains_key(relative_path))
            .unwrap_or(false);

    if !should_send {
        return;
    }

    if let Some(workspace_root) = current_workspace_root() {
        let full_path = format!("{}/{}", workspace_root.trim_end_matches('/'), relative_path);
        if let Ok(content) = host::fs::read_file(&full_path) {
            session::with_session_mut(|s| {
                // Recreate doc with new content and send full state
                let doc = file_doc::create_file_doc(&content);
                let state = file_doc::encode_full_state(&doc);
                let doc_id = wire::file_doc_id(&s.namespace_id, relative_path);
                let frame = wire::frame_binary(&doc_id, &state);
                let _ = host::ws::send_binary(&frame);
                s.open_files.insert(relative_path.to_string(), doc);
            });
        }
    }
}

fn handle_file_created(relative_path: &str) {
    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);
    if !is_host {
        return;
    }

    let parent = std::path::Path::new(relative_path).parent().and_then(|p| {
        let s = p.to_string_lossy().to_string();
        if s.is_empty() { None } else { Some(s) }
    });

    session::with_session_mut(|s| {
        manifest::add_entry(
            &s.manifest_doc,
            relative_path,
            parent.as_deref(),
            0,
            "text/markdown",
        );
        let state = manifest::encode_full_state(&s.manifest_doc);
        let doc_id = wire::manifest_doc_id(&s.namespace_id);
        let frame = wire::frame_binary(&doc_id, &state);
        let _ = host::ws::send_binary(&frame);
    });
}

fn handle_file_deleted(relative_path: &str) {
    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);
    if !is_host {
        return;
    }

    session::with_session_mut(|s| {
        manifest::remove_entry(&s.manifest_doc, relative_path);
        s.open_files.remove(relative_path);
        let state = manifest::encode_full_state(&s.manifest_doc);
        let doc_id = wire::manifest_doc_id(&s.namespace_id);
        let frame = wire::frame_binary(&doc_id, &state);
        let _ = host::ws::send_binary(&frame);
    });
}

fn handle_file_renamed(old_path: &str, new_path: &str) {
    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);
    if !is_host {
        return;
    }

    let parent = std::path::Path::new(new_path).parent().and_then(|p| {
        let s = p.to_string_lossy().to_string();
        if s.is_empty() { None } else { Some(s) }
    });

    session::with_session_mut(|s| {
        manifest::remove_entry(&s.manifest_doc, old_path);
        manifest::add_entry(
            &s.manifest_doc,
            new_path,
            parent.as_deref(),
            0,
            "text/markdown",
        );

        // Move file doc if it was open
        if let Some(doc) = s.open_files.remove(old_path) {
            s.open_files.insert(new_path.to_string(), doc);
        }

        let state = manifest::encode_full_state(&s.manifest_doc);
        let doc_id = wire::manifest_doc_id(&s.namespace_id);
        let frame = wire::frame_binary(&doc_id, &state);
        let _ = host::ws::send_binary(&frame);
    });
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = load_extism_config();
    let runtime = load_runtime_context();
    Ok(serde_json::to_string(&effective_config(&config, &runtime))?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let incoming: JsonValue = serde_json::from_str(&input)?;
    let mut config = load_extism_config();
    apply_config_patch(&mut config, &incoming);
    save_extism_config(&config);
    Ok(String::new())
}

// ============================================================================
// WebSocket message handlers
// ============================================================================

#[plugin_fn]
pub fn handle_binary_message(input: Vec<u8>) -> FnResult<Vec<u8>> {
    if let Some((doc_id, payload)) = wire::unframe_binary(&input) {
        if doc_id.starts_with("manifest:") {
            session::with_session_mut(|s| {
                manifest::apply_update(&s.manifest_doc, payload);
            });
            let _ = host::events::emit(r#"{"type":"manifest_updated"}"#);
        } else if let Some(rest) = doc_id.strip_prefix("file:") {
            // Extract path: file:{ns_id}/{path}
            if let Some((_ns, path)) = rest.split_once('/') {
                session::with_session_mut(|s| {
                    if let Some(doc) = s.open_files.get(path) {
                        file_doc::apply_update(doc, payload);
                    } else {
                        // File doc arriving for the first time (e.g., from host push)
                        let doc = yrs::Doc::new();
                        file_doc::apply_update(&doc, payload);
                        s.open_files.insert(path.to_string(), doc);
                    }
                });
                let _ = host::events::emit(&format!(
                    r#"{{"type":"file_content_updated","path":"{}"}}"#,
                    path.replace('"', r#"\""#)
                ));
            }
        }
    }

    Ok(Vec::new())
}

#[plugin_fn]
pub fn handle_text_message(input: String) -> FnResult<Vec<u8>> {
    if let Some(msg) = wire::parse_control_message(&input) {
        match msg {
            wire::ControlMessage::FileRequested { path, .. } => {
                // Host: someone wants a file, push it
                let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);
                if is_host {
                    handle_file_opened(&path);
                }
            }
            wire::ControlMessage::FileReady { path } => {
                session::with_session_mut(|s| {
                    s.pending_requests.remove(&path);
                });
                let _ = host::events::emit(&format!(
                    r#"{{"type":"file_ready","path":"{}"}}"#,
                    path.replace('"', r#"\""#)
                ));
            }
            wire::ControlMessage::PeerJoined {
                guest_id,
                peer_count,
            } => {
                let _ = host::events::emit(&format!(
                    r#"{{"type":"peer_joined","guest_id":"{}","peer_count":{}}}"#,
                    guest_id.replace('"', r#"\""#),
                    peer_count
                ));
            }
            wire::ControlMessage::PeerLeft {
                guest_id,
                peer_count,
            } => {
                let _ = host::events::emit(&format!(
                    r#"{{"type":"peer_left","guest_id":"{}","peer_count":{}}}"#,
                    guest_id.replace('"', r#"\""#),
                    peer_count
                ));
            }
            wire::ControlMessage::SessionEnded => {
                let is_guest =
                    session::with_session(|s| s.role == ShareRole::Guest).unwrap_or(false);
                if is_guest {
                    let _ = handle_end_share_session(&JsonValue::Null);
                }
                let _ = host::events::emit(r#"{"type":"session_ended"}"#);
            }
        }
    }

    Ok(Vec::new())
}

#[plugin_fn]
pub fn on_connected(_input: String) -> FnResult<Vec<u8>> {
    let is_host = session::with_session(|s| s.role == ShareRole::Host).unwrap_or(false);
    if is_host {
        // Re-send manifest state in case connection was re-established
        let data = session::with_session(|s| {
            let state = manifest::encode_full_state(&s.manifest_doc);
            let doc_id = wire::manifest_doc_id(&s.namespace_id);
            wire::frame_binary(&doc_id, &state)
        });
        if let Some(frame) = data {
            let _ = host::ws::send_binary(&frame);
        }
    }
    let _ = host::events::emit(r#"{"type":"share_connected"}"#);
    Ok(Vec::new())
}

#[plugin_fn]
pub fn on_disconnected(_input: String) -> FnResult<Vec<u8>> {
    let _ = host::events::emit(r#"{"type":"share_disconnected"}"#);
    Ok(Vec::new())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_exposes_live_share_surface_only() {
        let manifest = build_manifest();
        assert_eq!(manifest.id, "diaryx.share");
        assert!(
            manifest
                .ui
                .iter()
                .any(|ui| ui.get("slot").and_then(|v| v.as_str()) == Some("SettingsTab"))
        );
        assert!(
            manifest
                .ui
                .iter()
                .any(|ui| ui.get("id").and_then(|v| v.as_str()) == Some("share"))
        );
        assert!(
            !manifest
                .commands
                .iter()
                .any(|command| command == "FinalizeJoinShareSession")
        );
    }

    #[test]
    fn manifest_does_not_request_sync_plugin_permissions() {
        let manifest = build_manifest();
        let defaults = manifest
            .requested_permissions
            .as_ref()
            .map(|perms| &perms.defaults);
        // Should not have execute_commands for diaryx.sync
        let execute_commands = defaults
            .and_then(|d| d.as_object())
            .and_then(|d| d.get("execute_commands"));
        assert!(execute_commands.is_none());
    }

    #[test]
    fn apply_config_patch_clears_and_sets_values() {
        let mut cfg = ShareExtismConfig {
            server_url: Some("https://old.example".to_string()),
            auth_token: Some("old-token".to_string()),
            active_join_code: Some("oldcode".to_string()),
            share_read_only: Some(true),
            share_role: Some("host".to_string()),
            session_namespace_id: Some("ns-1".to_string()),
        };

        let patch = serde_json::json!({
            "server_url": null,
            "auth_token": "  ",
            "active_join_code": " newcode ",
            "share_read_only": null,
            "share_role": "guest",
            "session_namespace_id": " ns-2 "
        });
        apply_config_patch(&mut cfg, &patch);

        assert_eq!(cfg.server_url, None);
        assert_eq!(cfg.auth_token, None);
        assert_eq!(cfg.active_join_code.as_deref(), Some("NEWCODE"));
        assert_eq!(cfg.share_read_only, None);
        assert_eq!(cfg.share_role.as_deref(), Some("guest"));
        assert_eq!(cfg.session_namespace_id.as_deref(), Some("ns-2"));
    }

    #[test]
    fn normalize_server_base_strips_sync_suffixes() {
        assert_eq!(
            normalize_server_base("https://example.com/sync2"),
            "https://example.com"
        );
        assert_eq!(
            normalize_server_base("https://example.com/sync"),
            "https://example.com"
        );
        assert_eq!(
            normalize_server_base("https://example.com/"),
            "https://example.com"
        );
    }
}
