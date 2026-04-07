//! Structured logging via the host's logging system.

use super::*;

/// Log a message at the given level.
///
/// Valid levels: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`.
pub fn log(level: &str, message: &str) {
    let input = serde_json::json!({ "level": level, "message": message }).to_string();
    let _ = unsafe { host_log(input) };
}

/// Log at trace level.
pub fn trace(message: &str) {
    log("trace", message);
}

/// Log at debug level.
pub fn debug(message: &str) {
    log("debug", message);
}

/// Log at info level.
pub fn info(message: &str) {
    log("info", message);
}

/// Log at warn level.
pub fn warn(message: &str) {
    log("warn", message);
}

/// Log at error level.
pub fn error(message: &str) {
    log("error", message);
}
