//! User-provided file requests.
//!
//! Requires the `files` feature. Allows the plugin to request files
//! that the user has provided through the host UI.

use super::*;

/// Request a user-provided file by key.
///
/// Returns the raw file bytes, or an error if the file is not available.
pub fn request(key: &str) -> Result<Vec<u8>, String> {
    let input = serde_json::json!({ "key": key }).to_string();
    let result = unsafe { host_request_file(input) }
        .map_err(|e| format!("host_request_file failed: {e}"))?;
    Ok(result.into_bytes())
}
