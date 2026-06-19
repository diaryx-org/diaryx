//! Utility functions for WASM.

use std::future::Future;
use std::pin::Pin;

use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

/// Funnel exported async work through a **single** `future_to_promise`
/// monomorphization by erasing the future type to a boxed trait object.
///
/// `wasm-bindgen` generates a distinct `future_to_promise::<F>` instantiation
/// for every exported `async fn` (each has a unique opaque future type `F`),
/// emitting ~1.5–4 KB of promise-spawning/waker glue per function. Routing all
/// of them through this helper makes `F` identical
/// (`Pin<Box<dyn Future<Output = Result<JsValue, JsValue>>>>`), so the glue is
/// compiled once. The only per-call code left is the tiny box coercion.
///
/// Callers become `pub fn foo(..) -> Promise` whose body is
/// `let state = self.state.clone(); promise(async move { .. })` — the clone is
/// required because the future must be `'static`.
pub(crate) fn promise(fut: impl Future<Output = Result<JsValue, JsValue>> + 'static) -> Promise {
    let boxed: Pin<Box<dyn Future<Output = Result<JsValue, JsValue>>>> = Box::pin(fut);
    future_to_promise(boxed)
}

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
