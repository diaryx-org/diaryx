//! Timestamp and time functions from the host.

use super::*;

/// Get the current timestamp in milliseconds since the Unix epoch.
pub fn timestamp_millis() -> Result<u64, String> {
    let result = unsafe { host_get_timestamp(String::new()) }
        .map_err(|e| format!("host_get_timestamp failed: {e}"))?;
    result
        .trim()
        .parse::<u64>()
        .map_err(|e| format!("Failed to parse timestamp: {e}"))
}

/// Get the current local time as an RFC 3339 string (e.g. `"2024-01-15T10:30:00-07:00"`).
pub fn now_rfc3339() -> Result<String, String> {
    unsafe { host_get_now(String::new()) }.map_err(|e| format!("host_get_now failed: {e}"))
}
