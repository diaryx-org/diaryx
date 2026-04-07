//! Namespace object operations via the host runtime.
//!
//! Provides functions for listing namespaces, uploading and deleting objects
//! in namespaces, and syncing audience access levels. These operations go
//! through the host rather than direct HTTP, so plugins don't need HTTP
//! permissions for server operations.
//!
//! Requires the `namespaces` feature.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

use super::*;

/// Metadata for a single object in a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

/// Entry returned by `list_namespaces`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceEntry {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// List all namespaces owned by the authenticated user.
pub fn list_namespaces() -> Result<Vec<NamespaceEntry>, String> {
    let result = unsafe { host_namespace_list("{}".to_string()) }
        .map_err(|e| format!("host_namespace_list failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse list_namespaces response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    serde_json::from_value(parsed)
        .map_err(|e| format!("Failed to decode list_namespaces response: {e}"))
}

/// Download an object from a namespace as raw bytes.
pub fn get_object(ns_id: &str, key: &str) -> Result<Vec<u8>, String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
        "key": key,
    });
    let result = unsafe { host_namespace_get_object(input.to_string()) }
        .map_err(|e| format!("host_namespace_get_object failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse get_object response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    let data = parsed
        .get("data")
        .and_then(|v| v.as_str())
        .ok_or("Missing data in get_object response")?;
    BASE64
        .decode(data)
        .map_err(|e| format!("Failed to decode get_object response: {e}"))
}

/// Upload an object to a namespace.
pub fn put_object(
    ns_id: &str,
    key: &str,
    bytes: &[u8],
    mime_type: &str,
    audience: &str,
) -> Result<(), String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
        "key": key,
        "body_base64": BASE64.encode(bytes),
        "mime_type": mime_type,
        "audience": audience,
    });
    let result = unsafe { host_namespace_put_object(input.to_string()) }
        .map_err(|e| format!("host_namespace_put_object failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse put_object response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(())
}

/// Delete a single object from a namespace.
pub fn delete_object(ns_id: &str, key: &str) -> Result<(), String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
        "key": key,
    });
    let result = unsafe { host_namespace_delete_object(input.to_string()) }
        .map_err(|e| format!("host_namespace_delete_object failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse delete_object response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(())
}

/// List all objects in a namespace.
pub fn list_objects(ns_id: &str) -> Result<Vec<ObjectMeta>, String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
    });
    let result = unsafe { host_namespace_list_objects(input.to_string()) }
        .map_err(|e| format!("host_namespace_list_objects failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse list_objects response: {e}"))
}

/// Trigger sending the email draft for an audience to all subscribers.
///
/// The draft must already be uploaded as `_email_draft/{audience}.html`
/// in the namespace's object store. The server reads the draft, sends to
/// all active subscribers via Resend, writes a send receipt, and deletes
/// the draft.
pub fn send_audience_email(
    ns_id: &str,
    audience: &str,
    subject: &str,
    reply_to: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut input = serde_json::json!({
        "ns_id": ns_id,
        "audience": audience,
        "subject": subject,
    });
    if let Some(rt) = reply_to {
        input["reply_to"] = serde_json::Value::String(rt.to_string());
    }
    let result = unsafe { host_namespace_send_email(input.to_string()) }
        .map_err(|e| format!("host_namespace_send_email failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse send_email response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(parsed)
}

/// Sync an audience's access level on the server.
pub fn sync_audience(ns_id: &str, audience: &str, access: &str) -> Result<(), String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
        "audience": audience,
        "access": access,
    });
    let result = unsafe { host_namespace_sync_audience(input.to_string()) }
        .map_err(|e| format!("host_namespace_sync_audience failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse sync_audience response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(())
}
