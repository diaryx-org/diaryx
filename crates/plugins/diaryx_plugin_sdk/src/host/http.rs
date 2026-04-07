//! HTTP request support via the host runtime.
//!
//! Requires the `http` feature and the `http` permission in the plugin manifest.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;

use super::*;

/// Response from an HTTP request.
#[derive(Debug, Clone, Deserialize)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body as a UTF-8 string.
    pub body: String,
    /// Base64-encoded binary response body, if present.
    #[serde(default)]
    pub body_base64: Option<String>,
}

impl HttpResponse {
    /// Decode the response body as bytes.
    ///
    /// If `body_base64` is present, decodes that; otherwise returns `body` as bytes.
    pub fn body_bytes(&self) -> Result<Vec<u8>, String> {
        if let Some(b64) = &self.body_base64 {
            BASE64
                .decode(b64)
                .map_err(|e| format!("Failed to decode response body: {e}"))
        } else {
            Ok(self.body.as_bytes().to_vec())
        }
    }
}

/// Optional request tuning.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpRequestOptions {
    /// Optional request timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

fn build_request_input(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    body_base64: Option<String>,
    options: HttpRequestOptions,
) -> serde_json::Value {
    let mut input = serde_json::json!({
        "url": url,
        "method": method,
        "headers": headers,
    });
    if let Some(b) = body {
        input["body"] = serde_json::Value::String(b.to_string());
    }
    if let Some(encoded) = body_base64 {
        input["body_base64"] = serde_json::Value::String(encoded);
    }
    if let Some(timeout_ms) = options.timeout_ms {
        input["timeout_ms"] = serde_json::Value::Number(timeout_ms.into());
    }
    input
}

/// Perform an HTTP request with an optional string body.
pub fn request(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<HttpResponse, String> {
    request_with_options(method, url, headers, body, HttpRequestOptions::default())
}

/// Perform an HTTP request with an optional string body and request options.
pub fn request_with_options(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
    options: HttpRequestOptions,
) -> Result<HttpResponse, String> {
    let input = build_request_input(method, url, headers, body, None, options);
    let result = unsafe { host_http_request(input.to_string()) }
        .map_err(|e| format!("host_http_request failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse HTTP response: {e}"))
}

/// Perform an HTTP request with a binary body.
pub fn request_binary(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Result<HttpResponse, String> {
    request_binary_with_options(method, url, headers, body, HttpRequestOptions::default())
}

/// Perform an HTTP request with a binary body and request options.
pub fn request_binary_with_options(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    options: HttpRequestOptions,
) -> Result<HttpResponse, String> {
    let encoded = BASE64.encode(body);
    let input = build_request_input(method, url, headers, None, Some(encoded), options);
    let result = unsafe { host_http_request(input.to_string()) }
        .map_err(|e| format!("host_http_request failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse HTTP response: {e}"))
}

/// Perform an HTTP request with a JSON body and parse the response body as JSON.
pub fn request_json(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    request_json_with_options(method, url, headers, body, HttpRequestOptions::default())
}

/// Perform an HTTP request with a JSON body and parse the response body as JSON.
pub fn request_json_with_options(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&serde_json::Value>,
    options: HttpRequestOptions,
) -> Result<serde_json::Value, String> {
    let mut header_map = headers.clone();
    if body.is_some() {
        header_map
            .entry("Content-Type".to_string())
            .or_insert_with(|| "application/json".to_string());
    }
    let body_str = body.map(|b| b.to_string());
    let resp = request_with_options(method, url, &header_map, body_str.as_deref(), options)?;
    serde_json::from_str(&resp.body).map_err(|e| format!("Failed to parse JSON response: {e}"))
}

/// Convenience: HTTP GET request.
pub fn get(url: &str, headers: &HashMap<String, String>) -> Result<HttpResponse, String> {
    request("GET", url, headers, None)
}

/// Convenience: HTTP POST request with a string body.
pub fn post(
    url: &str,
    headers: &HashMap<String, String>,
    body: &str,
) -> Result<HttpResponse, String> {
    request("POST", url, headers, Some(body))
}
