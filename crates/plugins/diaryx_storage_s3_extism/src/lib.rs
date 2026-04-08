//! Extism WASM guest plugin — S3-compatible storage as AsyncFileSystem.
//!
//! This plugin exposes S3 operations as commands that map 1:1 to the
//! `AsyncFileSystem` trait methods. Frontend or native adapters dispatch
//! these commands to create an S3-backed filesystem.

mod s3_ops;
mod sigv4;

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use s3_ops::S3Config;
use serde_json::Value as JsonValue;
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

thread_local! {
    static CONFIG: RefCell<Option<S3Config>> = const { RefCell::new(None) };
}

fn with_config<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&S3Config) -> R,
{
    CONFIG.with(|c| {
        let borrow = c.borrow();
        let config = borrow.as_ref().ok_or("S3 plugin not configured")?;
        Ok(f(config))
    })
}

// ---------------------------------------------------------------------------
// Plugin exports
// ---------------------------------------------------------------------------

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let m = GuestManifest::new(
        "diaryx.storage.s3",
        "S3 Storage",
        env!("CARGO_PKG_VERSION"),
        "S3-compatible object storage as a filesystem backend",
        vec!["custom_commands".into()],
    )
    .ui(vec![
        serde_json::json!({
            "slot": "StorageProvider",
            "id": "diaryx.storage.s3",
            "label": "Amazon S3",
            "icon": "cloud",
            "description": "Store files in an S3-compatible bucket"
        }),
        serde_json::json!({
            "slot": "SettingsTab",
            "id": "s3-storage-settings",
            "label": "S3 Storage",
            "icon": "cloud",
            "fields": [
                { "type": "Section", "label": "S3 Configuration" },
                { "type": "Text", "key": "bucket", "label": "Bucket", "placeholder": "my-diaryx-bucket" },
                { "type": "Text", "key": "region", "label": "Region", "placeholder": "us-east-1" },
                { "type": "Text", "key": "prefix", "label": "Prefix", "description": "Key prefix within the bucket (e.g., \"diaryx/workspace1/\")", "placeholder": "diaryx/" },
                { "type": "Text", "key": "endpoint", "label": "Custom Endpoint", "description": "For S3-compatible services: MinIO, Cloudflare R2, Backblaze B2, etc.", "placeholder": "https://s3.example.com" },
                { "type": "Text", "key": "access_key_id", "label": "Access Key ID", "placeholder": "AKIAIOSFODNN7EXAMPLE" },
                { "type": "Password", "key": "secret_access_key", "label": "Secret Access Key", "placeholder": "••••••••" },
                { "type": "Toggle", "key": "path_style", "label": "Use path-style addressing" },
                { "type": "Button", "label": "Test Connection", "command": "TestConnection", "variant": "outline" }
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
        "TestConnection".into(),
        "GetConfig".into(),
        "SetConfig".into(),
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "http_requests": { "include": ["all"], "exclude": [] },
            "plugin_storage": { "include": ["all"], "exclude": [] }
        }),
        reasons: [
            ("http_requests".to_string(), "Communicate with the configured S3-compatible object storage endpoint.".to_string()),
            ("plugin_storage".to_string(), "Persist S3 connection settings for the current workspace.".to_string()),
        ].into_iter().collect(),
    });
    Ok(serde_json::to_string(&m)?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    // Try to load config from storage
    if let Ok(Some(data)) = host::storage::get("s3_config") {
        if let Ok(config) = serde_json::from_slice::<S3Config>(&data) {
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
        Some(c) => Ok(serde_json::to_string(&c)?),
        None => Ok("{}".into()),
    }
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: S3Config = serde_json::from_str(&input)?;
    // Persist to plugin storage
    let data = serde_json::to_vec(&config)?;
    let _ = host::storage::set("s3_config", &data);
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
            match with_config(|c| s3_ops::read_file(c, path)) {
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
            match with_config(|c| s3_ops::write_file(c, path, content)) {
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
            match with_config(|c| s3_ops::delete_file(c, path)) {
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
            match with_config(|c| s3_ops::exists(c, path)) {
                Ok(Ok(exists)) => CommandResponse::ok(serde_json::json!({ "exists": exists })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "ListFiles" => {
            let dir = params.get("dir").and_then(|v| v.as_str()).unwrap_or("");
            match with_config(|c| s3_ops::list_files(c, dir)) {
                Ok(Ok(files)) => CommandResponse::ok(serde_json::json!({ "files": files })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "ListMdFiles" => {
            let dir = params.get("dir").and_then(|v| v.as_str()).unwrap_or("");
            match with_config(|c| s3_ops::list_md_files(c, dir)) {
                Ok(Ok(files)) => CommandResponse::ok(serde_json::json!({ "files": files })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "CreateDirAll" => {
            // No-op for S3 — directories are implicit via key prefixes
            CommandResponse::ok_empty()
        }

        "IsDir" => {
            let path = match params.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => return CommandResponse::err("Missing 'path' parameter"),
            };
            match with_config(|c| s3_ops::is_dir(c, path)) {
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
            match with_config(|c| s3_ops::move_file(c, from, to)) {
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
            match with_config(|c| s3_ops::read_binary(c, path)) {
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
            match with_config(|c| s3_ops::write_binary(c, path, &data)) {
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
            match with_config(|c| s3_ops::get_modified_time(c, path)) {
                Ok(Ok(time)) => CommandResponse::ok(serde_json::json!({ "time": time })),
                Ok(Err(e)) => CommandResponse::err(e),
                Err(e) => CommandResponse::err(e),
            }
        }

        "TestConnection" => match with_config(|c| s3_ops::test_connection(c)) {
            Ok(Ok(())) => CommandResponse::ok(serde_json::json!({ "connected": true })),
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

        "SetConfig" => match serde_json::from_value::<S3Config>(params.clone()) {
            Ok(config) => {
                let data = serde_json::to_vec(&config).unwrap_or_default();
                let _ = host::storage::set("s3_config", &data);
                CONFIG.with(|c| *c.borrow_mut() = Some(config));
                CommandResponse::ok_empty()
            }
            Err(e) => CommandResponse::err(format!("Invalid S3 config: {e}")),
        },

        _ => CommandResponse::err(format!("Unknown command: {command}")),
    }
}
