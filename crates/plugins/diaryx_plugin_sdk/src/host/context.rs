//! Runtime context from the host.
//!
//! Requires the `context` feature. Provides metadata about the running
//! environment (e.g., platform, version, workspace info).

use super::*;

/// Get the runtime context as a JSON value.
pub fn get() -> Result<serde_json::Value, String> {
    let raw = unsafe { host_get_runtime_context(String::new()) }
        .map_err(|e| format!("host_get_runtime_context failed: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse host_get_runtime_context response: {e}"))
}
