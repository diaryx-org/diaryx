//! Host function imports for the Extism guest.
//!
//! These are functions provided by the host (diaryx_extism or JS SDK) that
//! the guest calls to perform I/O operations. From the guest's perspective,
//! these are synchronous calls — the host handles any async work.

use extism_pdk::*;

// ============================================================================
// Host function declarations
// ============================================================================

#[host_fn]
extern "ExtismHost" {
    /// Log a message via the host's logging system.
    pub fn host_log(input: String) -> String;

    /// Read a file from the workspace as a string.
    pub fn host_read_file(input: String) -> String;

    /// List files recursively under a prefix. Returns JSON array of paths.
    pub fn host_list_files(input: String) -> String;

    /// Check if a file exists. Returns JSON boolean.
    pub fn host_file_exists(input: String) -> String;

    /// Write a text file to the workspace.
    pub fn host_write_file(input: String) -> String;

    /// Write binary content to a file (base64-encoded input).
    pub fn host_write_binary(input: String) -> String;

    /// Emit a sync event to the host (JSON event payload).
    pub fn host_emit_event(input: String) -> String;

    /// Load persisted CRDT state by key. Returns base64-encoded bytes or empty.
    pub fn host_storage_get(input: String) -> String;

    /// Persist CRDT state by key (base64-encoded bytes).
    pub fn host_storage_set(input: String) -> String;

    /// Get the current timestamp in milliseconds since epoch.
    pub fn host_get_timestamp(input: String) -> String;

    /// Optional forward-compatible bridge for plugin-initiated websocket ops.
    pub fn host_ws_request(input: String) -> String;

    /// Perform an HTTP request via the host runtime.
    pub fn host_http_request(input: String) -> String;
}

// ============================================================================
// Safe wrapper functions
// ============================================================================

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Log a message at the given level.
pub fn log_message(level: &str, message: &str) {
    let input = serde_json::json!({ "level": level, "message": message }).to_string();
    let _ = unsafe { host_log(input) };
}

/// Read a workspace file as a string.
pub fn read_file(path: &str) -> Result<String, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    unsafe { host_read_file(input) }.map_err(|e| format!("host_read_file failed: {e}"))
}

/// List files recursively under a prefix.
pub fn list_files(prefix: &str) -> Result<Vec<String>, String> {
    let input = serde_json::json!({ "prefix": prefix }).to_string();
    let result =
        unsafe { host_list_files(input) }.map_err(|e| format!("host_list_files failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse file list: {e}"))
}

/// Check if a file exists.
pub fn file_exists(path: &str) -> Result<bool, String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result =
        unsafe { host_file_exists(input) }.map_err(|e| format!("host_file_exists failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse exists result: {e}"))
}

/// Write a text file to the workspace.
pub fn write_file(path: &str, content: &str) -> Result<(), String> {
    let input = serde_json::json!({ "path": path, "content": content }).to_string();
    unsafe { host_write_file(input) }.map_err(|e| format!("host_write_file failed: {e}"))?;
    Ok(())
}

/// Write binary content to a file.
pub fn write_binary(path: &str, content: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(content);
    let input = serde_json::json!({ "path": path, "content": encoded }).to_string();
    unsafe { host_write_binary(input) }.map_err(|e| format!("host_write_binary failed: {e}"))?;
    Ok(())
}

/// Emit an event to the host.
pub fn emit_event(event_json: &str) -> Result<(), String> {
    let input = event_json.to_string();
    unsafe { host_emit_event(input) }.map_err(|e| format!("host_emit_event failed: {e}"))?;
    Ok(())
}

/// Load persisted data by key.
pub fn storage_get(key: &str) -> Result<Option<Vec<u8>>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result =
        unsafe { host_storage_get(input) }.map_err(|e| format!("host_storage_get failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }
    // Try to parse as JSON first (host may return {"data": "base64..."} or raw base64)
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&result) {
        if let Some(data_str) = obj.get("data").and_then(|v| v.as_str()) {
            if data_str.is_empty() {
                return Ok(None);
            }
            let bytes = BASE64
                .decode(data_str)
                .map_err(|e| format!("Failed to decode storage data: {e}"))?;
            return Ok(Some(bytes));
        }
        if obj.is_null() {
            return Ok(None);
        }
    }
    // Fall back to raw base64
    let bytes = BASE64
        .decode(&result)
        .map_err(|e| format!("Failed to decode storage data: {e}"))?;
    Ok(Some(bytes))
}

/// Persist data by key.
pub fn storage_set(key: &str, data: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(data);
    let input = serde_json::json!({ "key": key, "data": encoded }).to_string();
    unsafe { host_storage_set(input) }.map_err(|e| format!("host_storage_set failed: {e}"))?;
    Ok(())
}

/// Get the current timestamp in milliseconds.
pub fn get_timestamp() -> Result<u64, String> {
    let result = unsafe { host_get_timestamp(String::new()) }
        .map_err(|e| format!("host_get_timestamp failed: {e}"))?;
    result
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("Failed to parse timestamp: {e}"))
}

/// Forward-compatible websocket host request bridge.
///
/// Current browser/native sync keeps socket ownership in the host transport,
/// so this may be a no-op until ws handoff mode is enabled.
pub fn ws_request(payload: &str) -> Result<String, String> {
    unsafe { host_ws_request(payload.to_string()) }
        .map_err(|e| format!("host_ws_request failed: {e}"))
}

/// Perform an HTTP request via the host runtime and parse the JSON response.
pub fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body_json: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let header_map: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    let input = serde_json::json!({
        "url": url,
        "method": method,
        "headers": header_map,
        "body": body_json.map(|b| b.to_string()),
    })
    .to_string();
    let raw = unsafe { host_http_request(input) }
        .map_err(|e| format!("host_http_request failed: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse host_http_request: {e}"))
}
