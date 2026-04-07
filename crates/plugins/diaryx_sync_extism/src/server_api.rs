//! Namespace-based server API helpers for the generic resource backend.
//!
//! Most functions call the `/namespaces/…` endpoints. Namespace listing is
//! host-backed through `diaryx_plugin_sdk`.

use diaryx_plugin_sdk::host::namespace as host_namespace;
use serde_json::Value as JsonValue;

use crate::{
    auth_headers, http_error, http_request_binary_compat, http_request_compat, load_extism_config,
    parse_http_body, parse_http_body_json, parse_http_status, resolve_auth_token,
    resolve_server_url,
};

// ---------------------------------------------------------------------------
// Namespaces
// ---------------------------------------------------------------------------

/// POST /namespaces — create a namespace.
///
/// The server generates a UUID for the namespace ID.
/// The `name` is stored in metadata so it can be displayed in the UI.
pub fn create_namespace(params: &JsonValue, name: &str) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let response = http_request_compat(
        "POST",
        &format!("{server}/namespaces"),
        &headers,
        Some(serde_json::json!({
            "metadata": {
                "name": name,
                "kind": "workspace",
                "provider": "diaryx.sync"
            }
        })),
    )?;
    let status = parse_http_status(&response);
    if status != 201 && status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    parse_http_body_json(&response).ok_or_else(|| "Invalid namespace response".to_string())
}

/// List namespaces owned by the authenticated user via the host runtime.
pub fn list_namespaces(_params: &JsonValue) -> Result<JsonValue, String> {
    let entries = host_namespace::list_namespaces()?;
    serde_json::to_value(entries).map_err(|e| format!("Failed to serialize namespaces: {e}"))
}

// ---------------------------------------------------------------------------
// Objects
// ---------------------------------------------------------------------------

/// PUT /namespaces/{ns_id}/objects/{key} — store bytes under the given key.
pub fn put_object(
    params: &JsonValue,
    namespace_id: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let token = resolve_auth_token(params, &config);
    let mut headers: Vec<(String, String)> =
        vec![("Content-Type".to_string(), content_type.to_string())];
    if let Some(t) = &token {
        headers.push(("Authorization".to_string(), format!("Bearer {}", t)));
    }
    let response = http_request_binary_compat(
        "PUT",
        &format!("{server}/namespaces/{namespace_id}/objects/{key}"),
        &headers,
        body,
    )?;
    let status = parse_http_status(&response);
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    parse_http_body_json(&response).ok_or_else(|| "Invalid response".to_string())
}

/// GET /namespaces/{ns_id}/objects/{key} — retrieve bytes by key.
pub fn get_object(params: &JsonValue, namespace_id: &str, key: &str) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let response = http_request_compat(
        "GET",
        &format!("{server}/namespaces/{namespace_id}/objects/{key}"),
        &headers,
        None,
    )?;
    let status = parse_http_status(&response);
    if status == 404 {
        return Ok(JsonValue::Null);
    }
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    Ok(response)
}

/// DELETE /namespaces/{ns_id}/objects/{key} — delete an object.
pub fn delete_object(params: &JsonValue, namespace_id: &str, key: &str) -> Result<(), String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let response = http_request_compat(
        "DELETE",
        &format!("{server}/namespaces/{namespace_id}/objects/{key}"),
        &headers,
        None,
    )?;
    let status = parse_http_status(&response);
    if status != 204 && status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    Ok(())
}

/// GET /namespaces/{ns_id}/objects — list object metadata.
pub fn list_objects(params: &JsonValue, namespace_id: &str) -> Result<JsonValue, String> {
    let config = load_extism_config();
    let server = resolve_server_url(params, &config).ok_or("Missing server_url")?;
    let headers = auth_headers(resolve_auth_token(params, &config));
    let response = http_request_compat(
        "GET",
        &format!("{server}/namespaces/{namespace_id}/objects"),
        &headers,
        None,
    )?;
    let status = parse_http_status(&response);
    if status != 200 {
        return Err(http_error(status, &parse_http_body(&response)));
    }
    parse_http_body_json(&response).ok_or_else(|| "Invalid response".to_string())
}
