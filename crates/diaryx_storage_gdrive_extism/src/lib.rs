//! Extism WASM guest plugin — Google Drive storage as AsyncFileSystem.
//!
//! This plugin exposes Google Drive operations as commands that map 1:1 to the
//! `AsyncFileSystem` trait methods. Frontend or native adapters dispatch
//! these commands to create a Google Drive-backed filesystem.

mod gdrive_ops;
mod host_bridge;
mod multipart;

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

// ---------------------------------------------------------------------------
// Protocol types
// ---------------------------------------------------------------------------

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

#[derive(serde::Deserialize)]
struct CommandRequest {
    command: String,
    params: JsonValue,
}

#[derive(serde::Serialize)]
struct CommandResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl CommandResponse {
    fn ok(data: JsonValue) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
    fn ok_empty() -> Self {
        Self {
            success: true,
            data: None,
            error: None,
        }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin exports
// ---------------------------------------------------------------------------

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let m = GuestManifest {
        id: "diaryx.storage.gdrive".into(),
        name: "Google Drive Storage".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Google Drive as a filesystem backend".into(),
        capabilities: vec!["custom_commands".into()],
        ui: vec![
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
                "component": {
                    "type": "Iframe",
                    "component_id": "storage.gdrive.settings",
                }
            }),
        ],
        commands: vec![
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
            "RefreshToken".into(),
            "ExchangeToken".into(),
            "GetConfig".into(),
            "SetConfig".into(),
            "get_component_html".into(),
        ],
        cli: vec![],
    };
    Ok(serde_json::to_string(&m)?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    // Try to load config from storage
    if let Ok(Some(data)) = host_bridge::storage_get("gdrive_config") {
        if let Ok(config) = serde_json::from_slice::<GDriveConfig>(&data) {
            CONFIG.with(|c| *c.borrow_mut() = Some(config));
        }
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
        Some(c) => Ok(serde_json::to_string(&serde_json::to_value(&c)?)?),
        None => Ok("{}".into()),
    }
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: GDriveConfig = serde_json::from_str(&input)?;
    let data = serde_json::to_vec(&config)?;
    let _ = host_bridge::storage_set("gdrive_config", &data);
    CONFIG.with(|c| *c.borrow_mut() = Some(config));
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

        "RefreshToken" => match with_config(|c| gdrive_ops::refresh_token(c)) {
            Ok(Ok(new_token)) => {
                // Update the stored config with new access token
                let _ = with_config_mut(|c| {
                    c.access_token = new_token.clone();
                    let data = serde_json::to_vec(c).unwrap_or_default();
                    let _ = host_bridge::storage_set("gdrive_config", &data);
                });
                CommandResponse::ok(serde_json::json!({ "access_token": new_token }))
            }
            Ok(Err(e)) => CommandResponse::err(e),
            Err(e) => CommandResponse::err(e),
        },

        "GetConfig" => {
            let config = CONFIG.with(|c| c.borrow().clone());
            match config {
                Some(c) => CommandResponse::ok(serde_json::to_value(c).unwrap_or_default()),
                None => CommandResponse::ok(serde_json::json!({})),
            }
        }

        "SetConfig" => match serde_json::from_value::<GDriveConfig>(params.clone()) {
            Ok(config) => {
                let data = serde_json::to_vec(&config).unwrap_or_default();
                let _ = host_bridge::storage_set("gdrive_config", &data);
                CONFIG.with(|c| *c.borrow_mut() = Some(config));
                CommandResponse::ok_empty()
            }
            Err(e) => CommandResponse::err(format!("Invalid GDrive config: {e}")),
        },

        "ExchangeToken" => {
            let code = match params.get("code").and_then(|v| v.as_str()) {
                Some(c) => c,
                None => return CommandResponse::err("Missing 'code' parameter"),
            };
            let redirect_uri = match params.get("redirect_uri").and_then(|v| v.as_str()) {
                Some(r) => r,
                None => return CommandResponse::err("Missing 'redirect_uri' parameter"),
            };
            match with_config(|c| gdrive_ops::exchange_token(c, code, redirect_uri)) {
                Ok(Ok((access_token, refresh_token))) => {
                    let _ = with_config_mut(|c| {
                        c.access_token = access_token.clone();
                        c.refresh_token = refresh_token.clone();
                        let data = serde_json::to_vec(c).unwrap_or_default();
                        let _ = host_bridge::storage_set("gdrive_config", &data);
                    });
                    CommandResponse::ok(serde_json::json!({
                        "access_token": access_token,
                        "refresh_token": refresh_token,
                    }))
                }
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "get_component_html" => {
            let component_id = params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match component_id {
                "storage.gdrive.settings" => CommandResponse::ok(serde_json::json!({
                    "html": include_str!("ui/settings.html"),
                })),
                _ => CommandResponse::err(format!("Unknown component: {component_id}")),
            }
        }

        _ => CommandResponse::err(format!("Unknown command: {command}")),
    }
}
