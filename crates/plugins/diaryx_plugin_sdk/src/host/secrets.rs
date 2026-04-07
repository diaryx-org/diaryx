//! Secret store for credentials and sensitive data.
//!
//! Requires the `secrets` feature. Secrets are stored separately from normal
//! plugin storage and may use platform-specific secure storage on the host.
//!
//! Host permission checks are currently enforced through the plugin-scoped
//! `plugin_storage` permission, matching how secret keys are sandboxed per
//! plugin in the host runtime.

use super::*;

/// Load a secret by key. Returns `None` if the key doesn't exist.
pub fn get(key: &str) -> Result<Option<String>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result =
        unsafe { host_secret_get(input) }.map_err(|e| format!("host_secret_get failed: {e}"))?;
    if result.is_empty() {
        return Ok(None);
    }
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&result) {
        if let Some(value) = obj.get("value").and_then(|v| v.as_str()) {
            if value.is_empty() {
                return Ok(None);
            }
            return Ok(Some(value.to_string()));
        }
    }
    Ok(Some(result))
}

/// Store a secret by key.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let input = serde_json::json!({ "key": key, "value": value }).to_string();
    unsafe { host_secret_set(input) }.map_err(|e| format!("host_secret_set failed: {e}"))?;
    Ok(())
}

/// Delete a secret by key.
pub fn delete(key: &str) -> Result<(), String> {
    let input = serde_json::json!({ "key": key }).to_string();
    unsafe { host_secret_delete(input) }.map_err(|e| format!("host_secret_delete failed: {e}"))?;
    Ok(())
}
