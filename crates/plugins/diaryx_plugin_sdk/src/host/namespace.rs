//! Namespace object operations via the host runtime.
//!
//! Provides functions for creating and listing namespaces, uploading,
//! downloading, deleting, and listing objects in namespaces, and syncing
//! audience access levels. These operations go through the host rather than
//! direct HTTP, so plugins don't need HTTP permissions for server operations.
//!
//! Requires the `namespaces` feature.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

use super::*;

/// Metadata for a single object in a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    #[serde(default)]
    pub namespace_id: Option<String>,
    pub key: String,
    #[serde(default)]
    pub r2_key: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub content_hash: Option<String>,
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

/// Optional filters for namespace object listings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListObjectsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
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

/// Create a namespace through the host runtime.
pub fn create_namespace(metadata: Option<&serde_json::Value>) -> Result<NamespaceEntry, String> {
    let mut input = serde_json::json!({});
    if let Some(metadata) = metadata {
        input["metadata"] = metadata.clone();
    }
    let result = unsafe { host_namespace_create(input.to_string()) }
        .map_err(|e| format!("host_namespace_create failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse create_namespace response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    serde_json::from_value(parsed)
        .map_err(|e| format!("Failed to decode create_namespace response: {e}"))
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

/// A single entry in a batch-get response.
#[derive(Debug, Clone)]
pub struct BatchGetEntry {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// Result of a batch object download.
#[derive(Debug, Clone, Default)]
pub struct BatchGetResult {
    pub objects: std::collections::HashMap<String, BatchGetEntry>,
    pub errors: std::collections::HashMap<String, String>,
}

/// Download multiple objects from a namespace in a single request.
pub fn get_objects_batch(ns_id: &str, keys: &[String]) -> Result<BatchGetResult, String> {
    let input = serde_json::json!({
        "ns_id": ns_id,
        "keys": keys,
    });
    let result = unsafe { host_namespace_get_objects_batch(input.to_string()) }
        .map_err(|e| format!("host_namespace_get_objects_batch failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse get_objects_batch response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }

    let mut batch = BatchGetResult::default();

    if let Some(objects) = parsed.get("objects").and_then(|v| v.as_object()) {
        for (key, entry) in objects {
            let data = entry
                .get("data")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("Missing data for key {key}"))?;
            let mime_type = entry
                .get("mime_type")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream")
                .to_string();
            let bytes = BASE64
                .decode(data)
                .map_err(|e| format!("Failed to decode base64 for {key}: {e}"))?;
            batch
                .objects
                .insert(key.clone(), BatchGetEntry { bytes, mime_type });
        }
    }

    if let Some(errors) = parsed.get("errors").and_then(|v| v.as_object()) {
        for (key, msg) in errors {
            batch.errors.insert(
                key.clone(),
                msg.as_str().unwrap_or("unknown error").to_string(),
            );
        }
    }

    Ok(batch)
}

/// Upload an object to a namespace with an optional audience tag.
pub fn put_object_with_audience(
    ns_id: &str,
    key: &str,
    bytes: &[u8],
    mime_type: &str,
    audience: Option<&str>,
) -> Result<(), String> {
    let mut input = serde_json::json!({
        "ns_id": ns_id,
        "key": key,
        "body_base64": BASE64.encode(bytes),
        "mime_type": mime_type,
    });
    if let Some(audience) = audience {
        input["audience"] = serde_json::Value::String(audience.to_string());
    }
    let result = unsafe { host_namespace_put_object(input.to_string()) }
        .map_err(|e| format!("host_namespace_put_object failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse put_object response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    Ok(())
}

/// Upload an audience-scoped object to a namespace.
pub fn put_object(
    ns_id: &str,
    key: &str,
    bytes: &[u8],
    mime_type: &str,
    audience: &str,
) -> Result<(), String> {
    put_object_with_audience(ns_id, key, bytes, mime_type, Some(audience))
}

/// Upload an owner-only object to a namespace.
pub fn put_private_object(
    ns_id: &str,
    key: &str,
    bytes: &[u8],
    mime_type: &str,
) -> Result<(), String> {
    put_object_with_audience(ns_id, key, bytes, mime_type, None)
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
    list_objects_with_options(ns_id, ListObjectsOptions::default())
}

/// List objects in a namespace with optional filtering and pagination.
pub fn list_objects_with_options(
    ns_id: &str,
    options: ListObjectsOptions,
) -> Result<Vec<ObjectMeta>, String> {
    let mut input = serde_json::to_value(options)
        .map_err(|e| format!("Failed to serialize list_objects options: {e}"))?;
    input["ns_id"] = serde_json::Value::String(ns_id.to_string());
    let result = unsafe { host_namespace_list_objects(input.to_string()) }
        .map_err(|e| format!("host_namespace_list_objects failed: {e}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse list_objects response: {e}"))?;
    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(err.to_string());
    }
    serde_json::from_value(parsed)
        .map_err(|e| format!("Failed to decode list_objects response: {e}"))
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
