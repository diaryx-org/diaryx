//! Extism WASM guest plugin — Google Drive storage as AsyncFileSystem.
//!
//! This plugin exposes Google Drive operations as commands that map 1:1 to the
//! `AsyncFileSystem` trait methods. Frontend or native adapters dispatch
//! these commands to create a Google Drive-backed filesystem.

mod gdrive_ops;
mod multipart;

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use gdrive_ops::GDriveConfig;
use serde_json::Value as JsonValue;
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

thread_local! {
    static CONFIG: RefCell<Option<GDriveConfig>> = const { RefCell::new(None) };
}

const CONFIG_STORAGE_KEY: &str = "gdrive_config";
const ACCESS_TOKEN_SECRET_KEY: &str = "gdrive_access_token";
const REFRESH_TOKEN_SECRET_KEY: &str = "gdrive_refresh_token";

fn default_config() -> GDriveConfig {
    GDriveConfig {
        root_folder_id: "root".to_string(),
        ..GDriveConfig::default()
    }
}

fn with_config<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&GDriveConfig) -> R,
{
    CONFIG.with(|c| {
        let borrow = c.borrow();
        let config = borrow
            .as_ref()
            .ok_or("Google Drive plugin not configured")?;
        Ok(f(config))
    })
}

fn with_config_mut<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut GDriveConfig) -> R,
{
    CONFIG.with(|c| {
        let mut borrow = c.borrow_mut();
        let config = borrow
            .as_mut()
            .ok_or("Google Drive plugin not configured")?;
        Ok(f(config))
    })
}

fn persist_config(config: &GDriveConfig) -> Result<(), String> {
    if config.access_token.is_empty() {
        host::secrets::delete(ACCESS_TOKEN_SECRET_KEY)?;
    } else {
        host::secrets::set(ACCESS_TOKEN_SECRET_KEY, &config.access_token)?;
    }

    if config.refresh_token.is_empty() {
        host::secrets::delete(REFRESH_TOKEN_SECRET_KEY)?;
    } else {
        host::secrets::set(REFRESH_TOKEN_SECRET_KEY, &config.refresh_token)?;
    }

    let mut stored = config.clone();
    stored.access_token.clear();
    stored.refresh_token.clear();
    let data = serde_json::to_vec(&stored).map_err(|e| format!("serialize config: {e}"))?;
    host::storage::set(CONFIG_STORAGE_KEY, &data)
}

fn load_persisted_config() -> Result<Option<GDriveConfig>, String> {
    let Some(data) = host::storage::get(CONFIG_STORAGE_KEY)? else {
        return Ok(None);
    };

    let mut config: GDriveConfig =
        serde_json::from_slice(&data).map_err(|e| format!("parse config: {e}"))?;

    let stored_access = host::secrets::get(ACCESS_TOKEN_SECRET_KEY)?;
    let stored_refresh = host::secrets::get(REFRESH_TOKEN_SECRET_KEY)?;
    let mut migrated = false;

    if let Some(access_token) = stored_access {
        config.access_token = access_token;
    } else if !config.access_token.is_empty() {
        migrated = true;
    }

    if let Some(refresh_token) = stored_refresh {
        config.refresh_token = refresh_token;
    } else if !config.refresh_token.is_empty() {
        migrated = true;
    }

    if migrated {
        persist_config(&config)?;
    }

    Ok(Some(config))
}

fn set_runtime_config(config: GDriveConfig) {
    CONFIG.with(|c| *c.borrow_mut() = Some(config));
}

fn current_config_or_default() -> GDriveConfig {
    CONFIG.with(|c| c.borrow().clone().unwrap_or_else(default_config))
}

fn merge_with_current_config(mut incoming: GDriveConfig) -> GDriveConfig {
    let current = current_config_or_default();
    if incoming.access_token.is_empty() {
        incoming.access_token = current.access_token;
    }
    if incoming.refresh_token.is_empty() {
        incoming.refresh_token = current.refresh_token;
    }
    if incoming.client_id.trim().is_empty() {
        incoming.client_id = current.client_id;
    }
    if incoming.root_folder_id.trim().is_empty() {
        incoming.root_folder_id = if current.root_folder_id.trim().is_empty() {
            "root".to_string()
        } else {
            current.root_folder_id
        };
    }
    incoming
}

fn view_config(config: &GDriveConfig) -> JsonValue {
    serde_json::json!({
        "client_id": config.client_id,
        "root_folder_id": if config.root_folder_id.is_empty() {
            "root"
        } else {
            config.root_folder_id.as_str()
        },
        "connected": !config.refresh_token.is_empty(),
    })
}

// ---------------------------------------------------------------------------
// Plugin exports
// ---------------------------------------------------------------------------

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let m = GuestManifest::new(
        "diaryx.storage.gdrive",
        "Google Drive Storage",
        env!("CARGO_PKG_VERSION"),
        "Google Drive as a filesystem backend",
        vec!["custom_commands".into()],
    )
    .ui(vec![
        serde_json::json!({
            "slot": "StorageProvider",
            "id": "diaryx.storage.gdrive",
            "label": "Google Drive",
            "icon": "cloud",
            "description": "Store files in Google Drive"
        }),
        serde_json::json!({
            "slot": "SettingsTab",
            "id": "gdrive-storage-settings",
            "label": "Google Drive",
            "icon": "cloud",
            "fields": [
                {
                    "type": "Section",
                    "label": "Connection",
                    "description": "Uses a host-managed Google OAuth client with PKCE. No client secret is required in plugin settings."
                },
                {
                    "type": "Button",
                    "label": "Connect Google Drive",
                    "command": "BeginOAuth"
                },
                {
                    "type": "Button",
                    "label": "Refresh Access Token",
                    "command": "RefreshToken",
                    "variant": "outline"
                },
                {
                    "type": "Button",
                    "label": "Disconnect",
                    "command": "Disconnect",
                    "variant": "destructive"
                },
                {
                    "type": "Section",
                    "label": "Storage Root",
                    "description": "Google Drive folder ID to use as the workspace root. Use \"root\" for top-level Drive."
                },
                {
                    "type": "Text",
                    "key": "root_folder_id",
                    "label": "Root Folder ID",
                    "placeholder": "root"
                }
            ]
        }),
    ])
    .commands(vec![
        "ReadFile".into(),
        "WriteFile".into(),
        "DeleteFile".into(),
        "Exists".into(),
        "ListFiles".into(),
        "ListMdFiles".into(),
        "CreateDirAll".into(),
        "IsDir".into(),
        "MoveFile".into(),
        "ReadBinary".into(),
        "WriteBinary".into(),
        "GetModifiedTime".into(),
        "BeginOAuth".into(),
        "CompleteOAuth".into(),
        "RefreshToken".into(),
        "Disconnect".into(),
        "ExchangeToken".into(),
        "GetConfig".into(),
        "SetConfig".into(),
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "http_requests": { "include": ["googleapis.com"], "exclude": [] },
            "plugin_storage": { "include": ["all"], "exclude": [] }
        }),
        reasons: [
            ("http_requests".to_string(), "Communicate with Google Drive and Google OAuth API endpoints.".to_string()),
            ("plugin_storage".to_string(), "Persist Google Drive settings and cached workspace metadata.".to_string()),
        ].into_iter().collect(),
    });
    Ok(serde_json::to_string(&m)?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    if let Ok(Some(config)) = load_persisted_config() {
        set_runtime_config(config);
    }
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    CONFIG.with(|c| *c.borrow_mut() = None);
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let resp = dispatch_command(&req.command, &req.params);
    Ok(serde_json::to_string(&resp)?)
}

#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = CONFIG.with(|c| c.borrow().clone());
    match config {
        Some(c) => Ok(serde_json::to_string(&view_config(&c))?),
        None => Ok(serde_json::to_string(&view_config(&default_config()))?),
    }
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: GDriveConfig = merge_with_current_config(serde_json::from_str(&input)?);
    persist_config(&config).map_err(Error::msg)?;
    set_runtime_config(config);
    Ok(String::new())
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

fn dispatch_command(command: &str, params: &JsonValue) -> CommandResponse {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    match command {
        "ReadFile" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::read_file(c, path)) {
                Ok(Ok(content)) => CommandResponse::ok(serde_json::json!({ "content": content })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "WriteFile" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            let content = match params.get("content").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return CommandResponse::err("Missing 'content' parameter"),
            };
            match with_config(|c| gdrive_ops::write_file(c, path, content)) {
                Ok(Ok(())) => CommandResponse::ok_empty(),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "DeleteFile" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::delete_file(c, path)) {
                Ok(Ok(())) => CommandResponse::ok_empty(),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "Exists" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::exists(c, path)) {
                Ok(Ok(exists)) => CommandResponse::ok(serde_json::json!({ "exists": exists })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "ListFiles" => {
            let dir = params.get("dir").and_then(|v| v.as_str()).unwrap_or("");
            match with_config(|c| gdrive_ops::list_files(c, dir)) {
                Ok(Ok(files)) => CommandResponse::ok(serde_json::json!({ "files": files })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "ListMdFiles" => {
            let dir = params.get("dir").and_then(|v| v.as_str()).unwrap_or("");
            match with_config(|c| gdrive_ops::list_md_files(c, dir)) {
                Ok(Ok(files)) => CommandResponse::ok(serde_json::json!({ "files": files })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "CreateDirAll" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::create_dir_all(c, path)) {
                Ok(Ok(())) => CommandResponse::ok_empty(),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "IsDir" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::is_dir(c, path)) {
                Ok(Ok(is_dir)) => CommandResponse::ok(serde_json::json!({ "isDir": is_dir })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "MoveFile" => {
            let from = match params.get("from").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'from' parameter"),
            };
            let to = match params.get("to").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'to' parameter"),
            };
            match with_config(|c| gdrive_ops::move_file(c, from, to)) {
                Ok(Ok(())) => CommandResponse::ok_empty(),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "ReadBinary" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::read_binary(c, path)) {
                Ok(Ok(data)) => {
                    let encoded = BASE64.encode(&data);
                    CommandResponse::ok(serde_json::json!({ "data": encoded }))
                }
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "WriteBinary" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            let data_b64 = match params.get("data").and_then(|v| v.as_str()) {
                Some(d) => d,
                None => return CommandResponse::err("Missing 'data' parameter (base64)"),
            };
            let data = match BASE64.decode(data_b64) {
                Ok(d) => d,
                Err(e) => return CommandResponse::err(format!("Invalid base64: {e}")),
            };
            match with_config(|c| gdrive_ops::write_binary(c, path, &data)) {
                Ok(Ok(())) => CommandResponse::ok_empty(),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "GetModifiedTime" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| gdrive_ops::get_modified_time(c, path)) {
                Ok(Ok(time)) => CommandResponse::ok(serde_json::json!({ "time": time })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "BeginOAuth" => {
            let client_id = params
                .get("client_id")
                .and_then(|v| v.as_str())
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    let fallback = current_config_or_default().client_id;
                    if fallback.trim().is_empty() {
                        None
                    } else {
                        Some(fallback)
                    }
                });
            let Some(client_id) = client_id else {
                return CommandResponse::err(
                    "Google Drive OAuth is not configured for this build. Set a host-managed client ID.",
                );
            };
            let redirect_uri = match params.get("redirect_uri").and_then(|v| v.as_str()) {
                Some(uri) if !uri.trim().is_empty() => uri,
                _ => return CommandResponse::err("Missing 'redirect_uri' parameter"),
            };
            let redirect_uri_prefix = params
                .get("redirect_uri_prefix")
                .and_then(|v| v.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(redirect_uri);
            let code_challenge = match params.get("code_challenge").and_then(|v| v.as_str()) {
                Some(value) if !value.trim().is_empty() => value,
                _ => return CommandResponse::err("Missing 'code_challenge' parameter"),
            };
            let scope = "https://www.googleapis.com/auth/drive.file";
            let auth_url = format!(
                "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&code_challenge={}&code_challenge_method=S256",
                gdrive_ops::uri_encode(&client_id),
                gdrive_ops::uri_encode(redirect_uri),
                gdrive_ops::uri_encode(scope),
                gdrive_ops::uri_encode(code_challenge),
            );
            CommandResponse::ok(serde_json::json!({
                "host_action": {
                    "type": "open-oauth",
                    "payload": {
                        "url": auth_url,
                        "redirect_uri_prefix": redirect_uri_prefix,
                    }
                },
                "follow_up": {
                    "command": "CompleteOAuth",
                    "params": {
                        "code_verifier": params.get("code_verifier").cloned().unwrap_or(JsonValue::Null),
                    }
                }
            }))
        }

        "RefreshToken" => match with_config(|c| gdrive_ops::refresh_token(c)) {
            Ok(Ok(new_token)) => {
                let persist_result = with_config_mut(|c| {
                    c.access_token = new_token.clone();
                    persist_config(c)
                });
                match persist_result {
                    Ok(Ok(())) => CommandResponse::ok(serde_json::json!({
                        "access_token": new_token,
                        "message": "Access token refreshed.",
                    })),
                    Ok(Err(e)) => CommandResponse::err(e),
                    Err(e) => CommandResponse::err(e),
                }
            }
            Ok(Err(e)) => CommandResponse::err(e),
            Err(e) => CommandResponse::err(e),
        },

        "GetConfig" => {
            let config = CONFIG.with(|c| c.borrow().clone());
            match config {
                Some(c) => CommandResponse::ok(view_config(&c)),
                None => CommandResponse::ok(view_config(&default_config())),
            }
        }

        "SetConfig" => match serde_json::from_value::<GDriveConfig>(params.clone()) {
            Ok(config) => {
                let config = merge_with_current_config(config);
                match persist_config(&config) {
                    Ok(()) => {
                        set_runtime_config(config);
                        CommandResponse::ok_empty()
                    }
                    Err(e) => CommandResponse::err(e),
                }
            }
            Err(e) => CommandResponse::err(format!("Invalid GDrive config: {e}")),
        },

        "CompleteOAuth" | "ExchangeToken" => {
            let code = match params.get("code").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return CommandResponse::err("Missing 'code' parameter"),
            };
            let redirect_uri = match params.get("redirect_uri").and_then(|v| v.as_str()) {
                Some(r) => r,
                None => return CommandResponse::err("Missing 'redirect_uri' parameter"),
            };
            let code_verifier = match params.get("code_verifier").and_then(|v| v.as_str()) {
                Some(v) if !v.trim().is_empty() => v,
                _ => return CommandResponse::err("Missing 'code_verifier' parameter"),
            };
            match with_config(|c| gdrive_ops::exchange_token(c, code, redirect_uri, code_verifier))
            {
                Ok(Ok((access_token, refresh_token))) => {
                    let persist_result = with_config_mut(|c| {
                        c.access_token = access_token.clone();
                        c.refresh_token = refresh_token.clone();
                        persist_config(c)
                    });
                    match persist_result {
                        Ok(Ok(())) => CommandResponse::ok(serde_json::json!({
                            "message": "Connected to Google Drive.",
                        })),
                        Ok(Err(e)) => CommandResponse::err(e),
                        Err(e) => CommandResponse::err(e),
                    }
                }
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "Disconnect" => {
            let persist_result = with_config_mut(|c| {
                c.access_token.clear();
                c.refresh_token.clear();
                persist_config(c)
            });
            match persist_result {
                Ok(Ok(())) => CommandResponse::ok(serde_json::json!({
                    "message": "Disconnected from Google Drive.",
                })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(_) => {
                    let mut config = current_config_or_default();
                    config.access_token.clear();
                    config.refresh_token.clear();
                    match persist_config(&config) {
                        Ok(()) => {
                            set_runtime_config(config);
                            CommandResponse::ok(serde_json::json!({
                                "message": "Disconnected from Google Drive.",
                            }))
                        }
                        Err(e) => CommandResponse::err(e),
                    }
                }
            }
        }

        _ => CommandResponse::err(format!("Unknown command: {command}")),
    }
}
