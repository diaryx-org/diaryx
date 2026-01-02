//! Utility functions for WASM.

use wasm_bindgen::prelude::*;

/// Generate an ISO 8601 timestamp for the current time.
#[wasm_bindgen]
pub fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Generate a formatted date string for the current date.
#[wasm_bindgen]
pub fn today_formatted(format: &str) -> String {
    chrono::Utc::now().format(format).to_string()
}
