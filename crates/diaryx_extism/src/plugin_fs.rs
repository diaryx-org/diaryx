//! `PluginFileSystem` — adapts an Extism storage plugin into an `AsyncFileSystem`.
//!
//! This allows CLI and Tauri to use S3/GDrive (or any storage plugin) as a
//! filesystem backend by routing `AsyncFileSystem` calls through plugin commands.

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::ExtismPluginAdapter;

/// An `AsyncFileSystem` backed by an Extism storage plugin.
///
/// All trait methods dispatch to plugin commands (ReadFile, WriteFile, etc.)
/// and parse the JSON response.
pub struct PluginFileSystem {
    plugin: Arc<Mutex<ExtismPluginAdapter>>,
}

impl PluginFileSystem {
    pub fn new(plugin: Arc<Mutex<ExtismPluginAdapter>>) -> Self {
        Self { plugin }
    }

    fn call_command(&self, command: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let plugin = self
            .plugin
            .lock()
            .map_err(|e| Error::new(ErrorKind::Other, format!("Plugin lock poisoned: {e}")))?;

        let input = serde_json::json!({
            "command": command,
            "params": params,
        });
        let input_str = serde_json::to_string(&input)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, format!("Serialize: {e}")))?;

        let result = plugin
            .call_guest("handle_command", &input_str)
            .map_err(|e| Error::new(ErrorKind::Other, format!("Plugin call failed: {e}")))?;

        let resp: serde_json::Value = serde_json::from_str(&result)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Parse response: {e}")))?;

        if resp.get("success").and_then(|v| v.as_bool()) == Some(true) {
            Ok(resp.get("data").cloned().unwrap_or(serde_json::Value::Null))
        } else {
            let err_msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown plugin error");
            if err_msg.starts_with("NotFound:") {
                Err(Error::new(ErrorKind::NotFound, err_msg))
            } else {
                Err(Error::new(ErrorKind::Other, err_msg))
            }
        }
    }
}

impl AsyncFileSystem for PluginFileSystem {
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let data = self.call_command(
                "ReadFile",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            data.get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "No content in response"))
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "WriteFile",
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "content": content,
                }),
            )?;
            Ok(())
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Check if exists first
            let data = self.call_command(
                "Exists",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            if data.get("exists").and_then(|v| v.as_bool()) == Some(true) {
                return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
            }
            self.call_command(
                "WriteFile",
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "content": content,
                }),
            )?;
            Ok(())
        })
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "DeleteFile",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            Ok(())
        })
    }

    fn list_md_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let data = self.call_command(
                "ListMdFiles",
                serde_json::json!({ "dir": dir.to_string_lossy() }),
            )?;
            parse_file_list(&data, dir)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.call_command(
                "Exists",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )
            .ok()
            .and_then(|d| d.get("exists").and_then(|v| v.as_bool()))
            .unwrap_or(false)
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "CreateDirAll",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            Ok(())
        })
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            self.call_command(
                "IsDir",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )
            .ok()
            .and_then(|d| d.get("isDir").and_then(|v| v.as_bool()))
            .unwrap_or(false)
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "MoveFile",
                serde_json::json!({
                    "from": from.to_string_lossy(),
                    "to": to.to_string_lossy(),
                }),
            )?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            use base64::Engine;
            let data = self.call_command(
                "ReadBinary",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            let b64 = data
                .get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::new(ErrorKind::InvalidData, "No data in response"))?;
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| Error::new(ErrorKind::InvalidData, format!("base64 decode: {e}")))
        })
    }

    fn hash_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let data = self.call_command(
                "HashFile",
                serde_json::json!({ "path": path.to_string_lossy() }),
            );
            if let Ok(data) = data
                && let Some(hash) = data.get("hash").and_then(|v| v.as_str())
            {
                return Ok(hash.to_string());
            }

            use sha2::{Digest, Sha256};
            let bytes = self.read_binary(path).await?;
            let hash = Sha256::digest(&bytes);
            Ok(hash.iter().fold(String::with_capacity(64), |mut s, b| {
                use std::fmt::Write;
                let _ = write!(s, "{:02x}", b);
                s
            }))
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(content);
            self.call_command(
                "WriteBinary",
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "data": encoded,
                }),
            )?;
            Ok(())
        })
    }

    fn list_files<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let data = self.call_command(
                "ListFiles",
                serde_json::json!({ "dir": dir.to_string_lossy() }),
            )?;
            parse_file_list(&data, dir)
        })
    }

    fn get_modified_time<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Option<i64>> {
        Box::pin(async move {
            self.call_command(
                "GetModifiedTime",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )
            .ok()
            .and_then(|d| d.get("time").and_then(|v| v.as_i64()))
        })
    }
}

/// Parse a file list response `{ "files": ["a.md", "b.md"] }` into PathBufs.
fn parse_file_list(data: &serde_json::Value, dir: &Path) -> Result<Vec<PathBuf>> {
    let files = data
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "No files in response"))?;
    Ok(files
        .iter()
        .filter_map(|v| v.as_str())
        .map(|name| dir.join(name))
        .collect())
}
