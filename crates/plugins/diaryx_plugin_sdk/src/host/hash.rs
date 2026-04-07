//! File hashing host functions.
//!
//! Provides native-speed SHA-256 hashing of workspace files, avoiding the
//! overhead of reading file bytes into the WASM sandbox just to hash them.

use super::*;

/// Compute the SHA-256 hash of a workspace file, returning the hex digest.
///
/// Returns `None` if the file does not exist or cannot be read.
pub fn hash_file(path: &str) -> Option<String> {
    let input = serde_json::json!({ "path": path }).to_string();
    let result = unsafe { host_hash_file(input) }.ok()?;
    if result.is_empty() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(&result).ok()?;
    parsed
        .get("hash")
        .and_then(|v| v.as_str())
        .map(String::from)
}
