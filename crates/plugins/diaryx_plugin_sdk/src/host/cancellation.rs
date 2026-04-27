//! Cooperative cancellation for long-running plugin operations.
//!
//! The host can flag an in-flight operation (identified by an opaque token,
//! typically passed in via command params) as cancelled. The plugin polls
//! [`is_cancelled`] between units of work — the WASM guest cannot be
//! preempted, so cancellation is cooperative.
//!
//! Tokens are scoped to the calling plugin: a host that flags
//! `"download:abc"` for plugin A does not affect plugin B.

use super::*;

/// Returns `true` if the host has flagged the given operation token as
/// cancelled. Returns `false` if the token is unknown or not cancelled.
///
/// Plugins should call this between batches of work and abort gracefully
/// (saving any partial progress) when it returns `true`.
pub fn is_cancelled(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let input = serde_json::json!({ "token": token }).to_string();
    let raw = match unsafe { host_is_cancelled(input) } {
        Ok(s) => s,
        Err(_) => return false,
    };
    if raw.trim().is_empty() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("cancelled").and_then(|c| c.as_bool()))
        .unwrap_or(false)
}
