//! Namespace-based server API helpers for the generic resource backend.
//!
//! All functions are host-backed through `diaryx_plugin_sdk` namespace
//! functions instead of generic HTTP requests.

use base64::Engine;
use diaryx_plugin_sdk::host::namespace as host_namespace;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Namespaces
// ---------------------------------------------------------------------------

/// POST /namespaces — create a namespace.
///
/// The server generates a UUID for the namespace ID.
/// The `name` is stored in metadata so it can be displayed in the UI.
pub fn create_namespace(_params: &JsonValue, name: &str) -> Result<JsonValue, String> {
    let metadata = serde_json::json!({
        "name": name,
        "kind": "workspace",
        "provider": "diaryx.sync"
    });
    let entry = host_namespace::create_namespace(Some(&metadata))?;
    serde_json::to_value(entry).map_err(|e| format!("Failed to serialize namespace: {e}"))
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
    _params: &JsonValue,
    namespace_id: &str,
    key: &str,
    body: &[u8],
    content_type: &str,
) -> Result<JsonValue, String> {
    host_namespace::put_private_object(namespace_id, key, body, content_type)?;
    Ok(serde_json::json!({ "ok": true }))
}

/// GET /namespaces/{ns_id}/objects/{key} — retrieve bytes by key.
pub fn get_object(_params: &JsonValue, namespace_id: &str, key: &str) -> Result<JsonValue, String> {
    match host_namespace::get_object(namespace_id, key) {
        Ok(bytes) => Ok(serde_json::json!({
            "status": 200,
            "headers": {},
            "body": "",
            "body_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
        })),
        Err(e) if is_not_found_error(&e) => Ok(JsonValue::Null),
        Err(e) => Err(e),
    }
}

/// DELETE /namespaces/{ns_id}/objects/{key} — delete an object.
pub fn delete_object(_params: &JsonValue, namespace_id: &str, key: &str) -> Result<(), String> {
    host_namespace::delete_object(namespace_id, key)
}

/// GET /namespaces/{ns_id}/objects — list object metadata.
pub fn list_objects(_params: &JsonValue, namespace_id: &str) -> Result<JsonValue, String> {
    let objects = host_namespace::list_objects(namespace_id)?;
    serde_json::to_value(objects).map_err(|e| format!("Failed to serialize objects: {e}"))
}

fn is_not_found_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("404") || lower.contains("not found")
}
