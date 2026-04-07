//! File I/O host functions.
//!
//! Provides read, write, list, delete, and existence checks for workspace files.
//! Both text and binary variants are available.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::*;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct FileMetadata {
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<i64>,
}

/// Read a workspace file as a UTF-8 string.
pub fn read_file(path: &str) -> Result<String, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_read_file(input) }.map_err(|e| format!("host_read_file failed: {e}"))?;
    // The host returns {"error": "..."} for I/O or permission errors instead of
    // trapping, so the guest can handle missing files gracefully.
    if result.starts_with("{\"error\"") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&result) {
            if let Some(msg) = v.get("error").and_then(|e| e.as_str()) {
                return Err(msg.to_string());
            }
        }
    }
    Ok(result)
}

/// Read a workspace file as raw bytes (binary).
pub fn read_binary(path: &str) -> Result<Vec<u8>, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_read_binary(input) }.map_err(|e| format!("host_read_binary failed: {e}"))?;
    if result.is_empty() {
        return Ok(Vec::new());
    }
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse binary response: {e}"))?;
    let data = parsed
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or("Missing data in binary response")?;
    BASE64
        .decode(data)
        .map_err(|e| format!("Failed to decode binary response: {e}"))
}

/// List files recursively under a prefix path.
///
/// Returns a `Vec` of relative file paths.
pub fn list_files(prefix: &str) -> Result<Vec<String>, String> {
    let input = serde_json::json!({ "prefix": prefix }).to_string();
    let result =
        unsafe { host_list_files(input) }.map_err(|e| format!("host_list_files failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse file list: {e}"))
}

/// Return the canonical file set for the current workspace.
pub fn workspace_file_set() -> Result<Vec<String>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let input = "{}".to_string();
        let result = unsafe { host_workspace_file_set(input) }
            .map_err(|e| format!("host_workspace_file_set failed: {e}"))?;
        return serde_json::from_str(&result)
            .map_err(|e| format!("Failed to parse workspace file set: {e}"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        Err("workspace_file_set is only available to WASM guest plugins".to_string())
    }
}

/// Check if a file exists in the workspace.
pub fn file_exists(path: &str) -> Result<bool, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_file_exists(input) }.map_err(|e| format!("host_file_exists failed: {e}"))?;
    // Host may return bare "true"/"false" or JSON boolean
    if result == "true" {
        return Ok(true);
    }
    if result == "false" {
        return Ok(false);
    }
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse exists result: {e}"))
}

/// Read lightweight metadata for a workspace file without loading its bytes.
pub fn file_metadata(path: &str) -> Result<FileMetadata, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let input = serde_json::json!({ "path": path }).to_string();
        let result = unsafe { host_file_metadata(input) }
            .map_err(|e| format!("host_file_metadata failed: {e}"))?;
        return serde_json::from_str(&result)
            .map_err(|e| format!("Failed to parse file metadata: {e}"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Native builds are used for tests and host-side tooling. Fall back to
        // existing host APIs so the new metadata helper doesn't introduce a
        // fresh unresolved import outside the WASM target.
        let exists = file_exists(path)?;
        if !exists {
            return Ok(FileMetadata::default());
        }
        let size_bytes = read_binary(path).ok().map(|bytes| bytes.len() as u64);
        return Ok(FileMetadata {
            exists,
            size_bytes,
            modified_at_ms: None,
        });
    }
}

/// Write a text file to the workspace.
pub fn write_file(path: &str, content: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path, "content": content }).to_string();
    let result =
        unsafe { host_write_file(input) }.map_err(|e| format!("host_write_file failed: {e}"))?;
    // The host returns a non-empty string on filesystem errors so that the
    // guest can handle them gracefully instead of aborting on a WASM trap.
    if !result.is_empty() {
        return Err(result);
    }
    Ok(())
}

/// Write binary content to a workspace file.
pub fn write_binary(path: &str, content: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(content);
    let input = serde_json::json!({ "path": path, "content": encoded }).to_string();
    let result = unsafe { host_write_binary(input) }
        .map_err(|e| format!("host_write_binary failed: {e}"))?;
    if !result.is_empty() {
        return Err(result);
    }
    Ok(())
}

/// Delete a file from the workspace.
pub fn delete_file(path: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_delete_file(input) }.map_err(|e| format!("host_delete_file failed: {e}"))?;
    if !result.is_empty() {
        return Err(result);
    }
    Ok(())
}
