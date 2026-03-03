//! Host function imports for the publish Extism guest.
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
    pub fn host_log(input: String) -> String;
    pub fn host_read_file(input: String) -> String;
    pub fn host_list_files(input: String) -> String;
    pub fn host_file_exists(input: String) -> String;
    pub fn host_write_file(input: String) -> String;
    pub fn host_write_binary(input: String) -> String;
    pub fn host_emit_event(input: String) -> String;
    pub fn host_storage_get(input: String) -> String;
    pub fn host_storage_set(input: String) -> String;
    pub fn host_get_timestamp(input: String) -> String;
    pub fn host_http_request(input: String) -> String;
    pub fn host_run_wasi_module(input: String) -> String;
}

// ============================================================================
// Safe wrapper functions
// ============================================================================

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

pub fn emit_event(event_json: &str) -> Result<(), String> {
    let input = event_json.to_string();
    unsafe { host_emit_event(input) }.map_err(|e| format!("host_emit_event failed: {e}"))?;
    Ok(())
}

pub fn storage_get(key: &str) -> Result<Option<Vec<u8>>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result =
        unsafe { host_storage_get(input) }.map_err(|e| format!("host_storage_get failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }
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
    let bytes = BASE64
        .decode(&result)
        .map_err(|e| format!("Failed to decode storage data: {e}"))?;
    Ok(Some(bytes))
}

pub fn storage_set(key: &str, data: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(data);
    let input = serde_json::json!({ "key": key, "data": encoded }).to_string();
    unsafe { host_storage_set(input) }.map_err(|e| format!("host_storage_set failed: {e}"))?;
    Ok(())
}

pub fn get_timestamp() -> Result<u64, String> {
    let result = unsafe { host_get_timestamp(String::new()) }
        .map_err(|e| format!("host_get_timestamp failed: {e}"))?;
    result
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("Failed to parse timestamp: {e}"))
}

/// Perform an HTTP request via the host.
pub fn http_request(
    url: &str,
    method: &str,
    headers: &std::collections::HashMap<String, String>,
    body: Option<&str>,
) -> Result<HttpResponse, String> {
    let mut input = serde_json::json!({
        "url": url,
        "method": method,
        "headers": headers,
    });
    if let Some(b) = body {
        input["body"] = serde_json::Value::String(b.to_string());
    }
    let result = unsafe { host_http_request(input.to_string()) }
        .map_err(|e| format!("host_http_request failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse HTTP response: {e}"))
}

#[derive(serde::Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
    #[serde(default)]
    pub body_base64: Option<String>,
}

/// Run a WASI module stored in plugin storage.
pub fn run_wasi_module(request: &WasiRunRequest) -> Result<WasiRunResult, String> {
    let input = serde_json::to_string(request)
        .map_err(|e| format!("Failed to serialize WASI request: {e}"))?;
    let result = unsafe { host_run_wasi_module(input) }
        .map_err(|e| format!("host_run_wasi_module failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse WASI result: {e}"))
}

#[derive(serde::Serialize)]
pub struct WasiRunRequest {
    pub module_key: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_files: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
pub struct WasiRunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub files: Option<std::collections::HashMap<String, String>>,
}
