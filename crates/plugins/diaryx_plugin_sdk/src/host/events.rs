//! Event emission to the host application.
//!
//! Requires the `events` feature. Plugins can emit events that the host
//! application will process (e.g., triggering UI updates, sync operations).

use super::*;

/// Emit a raw JSON event to the host.
pub fn emit(event_json: &str) -> Result<(), String> {
    let input = event_json.to_string();
    unsafe { host_emit_event(input) }.map_err(|e| format!("host_emit_event failed: {e}"))?;
    Ok(())
}

/// Emit a typed event to the host (serialized as JSON).
pub fn emit_typed<T: serde::Serialize>(event: &T) -> Result<(), String> {
    let json =
        serde_json::to_string(event).map_err(|e| format!("Failed to serialize event: {e}"))?;
    emit(&json)
}
