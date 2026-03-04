//! Host function imports for the import Extism guest.

use extism_pdk::*;

#[host_fn]
extern "ExtismHost" {
    pub fn host_log(input: String) -> String;
    pub fn host_read_file(input: String) -> String;
    pub fn host_list_files(input: String) -> String;
    pub fn host_file_exists(input: String) -> String;
    pub fn host_write_file(input: String) -> String;
    pub fn host_write_binary(input: String) -> String;
    pub fn host_delete_file(input: String) -> String;
    pub fn host_request_file(input: String) -> String;
}

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

pub fn log_message(level: &str, message: &str) {
    let input = serde_json::json!({ "level": level, "message": message }).to_string();
    let _ = unsafe { host_log(input) };
}

pub fn read_file(path: &str) -> Result<String, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    unsafe { host_read_file(input) }.map_err(|e| format!("host_read_file failed: {e}"))
}

pub fn list_files(prefix: &str) -> Result<Vec<String>, String> {
    let input = serde_json::json!({ "prefix": prefix }).to_string();
    let result =
        unsafe { host_list_files(input) }.map_err(|e| format!("host_list_files failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse file list: {e}"))
}

pub fn file_exists(path: &str) -> Result<bool, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_file_exists(input) }.map_err(|e| format!("host_file_exists failed: {e}"))?;

    if result == "true" {
        return Ok(true);
    }
    if result == "false" {
        return Ok(false);
    }

    serde_json::from_str(&result).map_err(|e| format!("Failed to parse exists result: {e}"))
}

pub fn write_file(path: &str, content: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path, "content": content }).to_string();
    unsafe { host_write_file(input) }.map_err(|e| format!("host_write_file failed: {e}"))?;
    Ok(())
}

pub fn write_binary(path: &str, content: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(content);
    let input = serde_json::json!({ "path": path, "content": encoded }).to_string();
    unsafe { host_write_binary(input) }.map_err(|e| format!("host_write_binary failed: {e}"))?;
    Ok(())
}

/// Request a user-provided file by key name.
///
/// Returns the raw bytes if the host has a file for this key, or `None`.
pub fn request_file(key: &str) -> Result<Option<Vec<u8>>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result = unsafe { host_request_file(input) }
        .map_err(|e| format!("host_request_file failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }
    // Parse {"data": "<base64>"} response
    let parsed: serde_json::Value =
        serde_json::from_str(&result).map_err(|e| format!("Failed to parse response: {e}"))?;
    let b64 = parsed
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'data' field in response")?;
    let bytes = BASE64
        .decode(b64)
        .map_err(|e| format!("Failed to decode base64: {e}"))?;
    Ok(Some(bytes))
}
