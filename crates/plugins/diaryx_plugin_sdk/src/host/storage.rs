//! Plugin-scoped key-value storage.
//!
//! Values are automatically namespaced by plugin ID on the host side.
//! Data is persisted as base64-encoded bytes.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::*;

/// Load a value by key. Returns `None` if the key doesn't exist.
pub fn get(key: &str) -> Result<Option<Vec<u8>>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result =
        unsafe { host_storage_get(input) }.map_err(|e| format!("host_storage_get failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }
    // Host may return {"data": "base64..."} or raw base64
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

/// Store a value by key.
pub fn set(key: &str, data: &[u8]) -> Result<(), String> {
    let encoded = BASE64.encode(data);
    let input = serde_json::json!({ "key": key, "data": encoded }).to_string();
    let result =
        unsafe { host_storage_set(input) }.map_err(|e| format!("host_storage_set failed: {e}"))?;
    if !result.is_empty() {
        return Err(result);
    }
    Ok(())
}

/// Load a value and deserialize it from JSON.
pub fn get_json<T: serde::de::DeserializeOwned>(key: &str) -> Result<Option<T>, String> {
    match get(key)? {
        Some(bytes) => {
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("Failed to deserialize storage value: {e}"))?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

/// Serialize a value to JSON and store it.
pub fn set_json<T: serde::Serialize>(key: &str, value: &T) -> Result<(), String> {
    let bytes =
        serde_json::to_vec(value).map_err(|e| format!("Failed to serialize storage value: {e}"))?;
    set(key, &bytes)
}

/// Delete a storage key by setting it to empty.
///
/// Note: The host storage trait has a `delete` method, but it's not exposed
/// as a host function yet. Setting to empty is the current convention.
pub fn delete(key: &str) -> Result<(), String> {
    set(key, &[])
}
