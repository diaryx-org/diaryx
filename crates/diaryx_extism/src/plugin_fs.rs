//! `PluginFileSystem` — adapts an Extism storage plugin into an `AsyncFileSystem`.
//!
//! All trait methods dispatch to plugin commands (ReadFile, WriteFile, etc.)
//! and parse the JSON response.

use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::ExtismPluginAdapter;

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
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
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

    fn read_dir<'a>(&'a self, dir: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        Box::pin(async move {
            let data = self.call_command(
                "ListFiles",
                serde_json::json!({ "dir": dir.to_string_lossy() }),
            )?;
            let paths = parse_file_list(&data, dir)?;
            // The plugin protocol does not expose per-entry file types yet;
            // assume files. Callers needing dir-vs-file should call
            // `metadata(path)` per entry.
            Ok(paths
                .into_iter()
                .map(|p| DirEntry::new(p, FileType::file()))
                .collect())
        })
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            // Use WriteFile for valid UTF-8 (text) and WriteBinary otherwise.
            if let Ok(text) = std::str::from_utf8(contents) {
                self.call_command(
                    "WriteFile",
                    serde_json::json!({
                        "path": path.to_string_lossy(),
                        "content": text,
                    }),
                )?;
            } else {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(contents);
                self.call_command(
                    "WriteBinary",
                    serde_json::json!({
                        "path": path.to_string_lossy(),
                        "data": encoded,
                    }),
                )?;
            }
            Ok(())
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "CreateDirAll",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            Ok(())
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

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.call_command(
                "DeleteFile",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            Ok(())
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        // Plugin protocol does not currently distinguish file/dir removal.
        self.remove_file(path)
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        self.remove_file(path)
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
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

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        Box::pin(async move {
            // Compose from Exists / IsDir / GetModifiedTime since the plugin
            // protocol has no `Metadata` command yet.
            let exists_resp = self.call_command(
                "Exists",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            let exists = exists_resp
                .get("exists")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !exists {
                return Err(Error::new(ErrorKind::NotFound, "Path not found"));
            }

            let is_dir_resp = self.call_command(
                "IsDir",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            let is_dir = is_dir_resp
                .get("isDir")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let modified = self
                .call_command(
                    "GetModifiedTime",
                    serde_json::json!({ "path": path.to_string_lossy() }),
                )
                .ok()
                .and_then(|d| d.get("time").and_then(|v| v.as_i64()))
                .and_then(|ms| {
                    let secs = ms / 1000;
                    let nanos = (ms.rem_euclid(1000)) as u32 * 1_000_000;
                    Some(std::time::UNIX_EPOCH + std::time::Duration::new(secs as u64, nanos))
                });

            let file_type = if is_dir {
                FileType::dir()
            } else {
                FileType::file()
            };

            // Plugin protocol does not expose a file-size primitive.
            Ok(Metadata::new(file_type, 0, modified))
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let data = self.call_command(
                "Exists",
                serde_json::json!({ "path": path.to_string_lossy() }),
            )?;
            if data.get("exists").and_then(|v| v.as_bool()) == Some(true) {
                return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
            }
            self.write(path, contents).await
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
