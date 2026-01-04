//! Error handling utilities for WASM bindings.

use wasm_bindgen::JsValue;

/// Extension trait for converting Results to JS-compatible errors.
pub trait IntoJsResult<T> {
    /// Convert to a Result with JsValue error.
    fn js_err(self) -> Result<T, JsValue>;
}

impl<T, E: std::fmt::Display> IntoJsResult<T> for Result<T, E> {
    fn js_err(self) -> Result<T, JsValue> {
        self.map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

/// Extension trait for converting Options to JS-compatible errors.
pub trait IntoJsOption<T> {
    /// Convert to a Result with JsValue error using the provided message.
    fn js_ok_or(self, msg: &str) -> Result<T, JsValue>;

    /// Convert to a Result with JsValue error using a formatted message.
    fn js_ok_or_else<F: FnOnce() -> String>(self, f: F) -> Result<T, JsValue>;
}

impl<T> IntoJsOption<T> for Option<T> {
    fn js_ok_or(self, msg: &str) -> Result<T, JsValue> {
        self.ok_or_else(|| JsValue::from_str(msg))
    }

    fn js_ok_or_else<F: FnOnce() -> String>(self, f: F) -> Result<T, JsValue> {
        self.ok_or_else(|| JsValue::from_str(&f()))
    }
}
