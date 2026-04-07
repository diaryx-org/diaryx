//! Inter-plugin command execution via the host bridge.
//!
//! Requires the `plugins` feature. Allows one plugin to call commands
//! on another plugin through the host's plugin registry.

use super::*;

/// Execute a command on another plugin.
///
/// Returns the command result data on success, or an error message on failure.
pub fn call(
    plugin_id: &str,
    command: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let input = serde_json::json!({
        "plugin_id": plugin_id,
        "command": command,
        "params": params,
    })
    .to_string();
    let raw = unsafe { host_plugin_command(input) }
        .map_err(|e| format!("host_plugin_command failed: {e}"))?;
    let response: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse host_plugin_command response: {e}"))?;
    if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
        Ok(response
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    } else {
        Err(response
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown plugin command error")
            .to_string())
    }
}
