//! Host function imports for the Google Drive storage Extism guest.

use extism_pdk::*;
use std::collections::HashMap;

#[host_fn]
extern "ExtismHost" {
    pub fn host_log(input: String) -> String;
    pub fn host_storage_get(input: String) -> String;
    pub fn host_storage_set(input: String) -> String;
    pub fn host_get_timestamp(input: String) -> String;
    pub fn host_http_request(input: String) -> String;
}

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

pub fn log_message(level: &str, message: &str) {
    let input = serde_json::json!({ "level": level, "message": message }).to_string();
    let _ = unsafe { host_log(input) };
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

#[derive(serde::Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    #[serde(default)]
    pub body_base64: Option<String>,
}

/// Perform an HTTP request with a string body.
pub fn http_request(
    url: &str,
    method: &str,
    headers: &HashMap<String, String>,
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

/// Perform an HTTP request with a binary body (base64-encoded).
pub fn http_request_binary(
    url: &str,
    method: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Result<HttpResponse, String> {
    let encoded = BASE64.encode(body);
    let input = serde_json::json!({
        "url": url,
        "method": method,
        "headers": headers,
        "body_base64": encoded,
    });
    let result = unsafe { host_http_request(input.to_string()) }
        .map_err(|e| format!("host_http_request failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse HTTP response: {e}"))
}

/// Decode base64 response body to bytes.
pub fn decode_response_body(resp: &HttpResponse) -> Result<Vec<u8>, String> {
    if let Some(b64) = &resp.body_base64 {
        BASE64
            .decode(b64)
            .map_err(|e| format!("Failed to decode response body: {e}"))
    } else {
        Ok(resp.body.as_bytes().to_vec())
    }
}
