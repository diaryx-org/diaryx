//! Proxy request support via the host runtime.
//!
//! Routes requests through the server's generic proxy service, which handles
//! credential management, tier gating, rate limiting, and usage metering.
//!
//! Requires the `proxy` feature.

use std::collections::HashMap;

use super::http::HttpResponse;
use super::*;

/// Send a request through a registered server proxy.
///
/// The server resolves credentials (platform key, user secret, or HMAC signing)
/// based on the proxy configuration. The plugin never sees the raw API key.
///
/// # Arguments
/// * `proxy_id` — registered proxy identifier (e.g., `"diaryx.ai"`)
/// * `path` — path appended to the proxy's upstream URL (e.g., `"chat/completions"`)
/// * `method` — HTTP method (e.g., `"POST"`)
/// * `headers` — additional request headers
/// * `body` — optional request body as string
pub fn request(
    proxy_id: &str,
    path: &str,
    method: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<HttpResponse, String> {
    let mut input = serde_json::json!({
        "proxy_id": proxy_id,
        "path": path,
        "method": method,
        "headers": headers,
    });
    if let Some(b) = body {
        input["body"] = serde_json::Value::String(b.to_string());
    }
    let result = unsafe { host_proxy_request(input.to_string()) }
        .map_err(|e| format!("host_proxy_request failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse proxy response: {e}"))
}

/// Send a request with a JSON body through a registered server proxy.
pub fn request_json(
    proxy_id: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut header_map = headers.clone();
    header_map
        .entry("Content-Type".to_string())
        .or_insert_with(|| "application/json".to_string());
    let resp = request(proxy_id, path, "POST", &header_map, Some(&body.to_string()))?;
    serde_json::from_str(&resp.body).map_err(|e| format!("Failed to parse JSON response: {e}"))
}
