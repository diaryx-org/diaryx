//! Browser-side publish: a [`NamespaceProvider`] backed by JavaScript
//! callbacks, so [`crate::backend::DiaryxBackend`] can drive
//! [`diaryx_core::publish::PublishService`] directly — no Extism plugin.
//!
//! The workspace filesystem lives in the WASM backend (in the worker), while
//! the namespace HTTP calls run on the main thread (where the session cookie /
//! `getServerUrl()` are available). The main thread passes a `provider` object
//! whose seven async methods perform the actual fetches; this module forwards
//! each [`NamespaceProvider`] call to the matching JS callback (Comlink marshals
//! the call back to the main thread).
//!
//! ## Expected `provider` shape (all async, may return a Promise)
//!
//! ```javascript
//! {
//!   listObjects:   async (nsId) => JSON.stringify([{ key, audience, content_hash }]),
//!   putObject:     async (nsId, key, bytes /* Uint8Array */, mime, audience, fileArk, sourceKey, objectKey, isIndex) => {},
//!   deleteObject:  async (nsId, key) => {},
//!   syncAudience:  async (nsId, audience, gatesJson /* string */) => {},
//!   listAudiences: async (nsId) => JSON.stringify(["public", ...]),
//!   deleteAudience:async (nsId, audience) => {},
//!   buildNamespace:async (nsId, baseUrl /* string|null */) => {},
//! }
//! ```

use async_trait::async_trait;
use diaryx_core::publish::{NamespaceProvider, ObjectMeta};
use diaryx_core::yaml;
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// Serialize a JSON string into a `fig` value tree (serde_json-free).
fn parse_json(json: &str, ctx: &str) -> Result<fig::Value, String> {
    fig::Document::parse(json.as_bytes(), fig::Format::Json)
        .and_then(|doc| doc.to_value())
        .map_err(|e| format!("{ctx} decode: {e}"))
}

/// Serialize a `yaml::Value` to a compact JSON string via `fig`.
fn to_json(value: &yaml::Value, ctx: &str) -> Result<String, String> {
    fig::Value::from(value)
        .serialize_with(fig::Format::Json, fig::SerializeOptions::compact())
        .map(|s| s.trim_end().to_string())
        .map_err(|e| format!("{ctx} encode: {e}"))
}

/// A [`NamespaceProvider`] that forwards every operation to JS callbacks.
pub struct JsNamespaceProvider {
    callbacks: JsValue,
}

impl JsNamespaceProvider {
    pub fn new(callbacks: JsValue) -> Self {
        Self { callbacks }
    }
}

fn opt_str(value: Option<&str>) -> JsValue {
    match value {
        Some(s) => JsValue::from_str(s),
        None => JsValue::NULL,
    }
}

fn js_err(name: &str, err: JsValue) -> String {
    let detail = err
        .as_string()
        .or_else(|| {
            Reflect::get(&err, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "unknown JS error".to_string());
    format!("{name}: {detail}")
}

/// Invoke `callbacks[name](..args)` and await the result if it is a Promise.
async fn call_cb(callbacks: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let func = Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
        .ok_or_else(|| format!("publish provider callback '{name}' not provided"))?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => func.call0(&this),
        1 => func.call1(&this, &args[0]),
        2 => func.call2(&this, &args[0], &args[1]),
        3 => func.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let array = Array::new();
            for arg in args {
                array.push(arg);
            }
            func.apply(&this, &array)
        }
    }
    .map_err(|e| js_err(name, e))?;

    if result.has_type::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise).await.map_err(|e| js_err(name, e))
    } else {
        Ok(result)
    }
}

#[async_trait(?Send)]
impl NamespaceProvider for JsNamespaceProvider {
    async fn list_objects(&self, ns_id: &str) -> Result<Vec<ObjectMeta>, String> {
        let result = call_cb(&self.callbacks, "listObjects", &[JsValue::from_str(ns_id)]).await?;
        let json = result
            .as_string()
            .ok_or_else(|| "listObjects: expected a JSON string".to_string())?;

        #[derive(fig::FromValue)]
        struct Row {
            key: String,
            #[fig(default)]
            audience: Option<String>,
            #[fig(default)]
            content_hash: Option<String>,
        }
        let value = parse_json(&json, "listObjects")?;
        let rows = <Vec<Row> as fig::FromValue>::from_value(&value)
            .map_err(|e| format!("listObjects decode: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| ObjectMeta {
                key: r.key,
                audience: r.audience,
                content_hash: r.content_hash,
            })
            .collect())
    }

    async fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
        file_ark: Option<&str>,
        source_key: Option<&str>,
        object_key: Option<&str>,
        is_index: bool,
    ) -> Result<(), String> {
        let bytes_js: JsValue = Uint8Array::from(bytes).into();
        call_cb(
            &self.callbacks,
            "putObject",
            &[
                JsValue::from_str(ns_id),
                JsValue::from_str(key),
                bytes_js,
                JsValue::from_str(mime_type),
                opt_str(audience),
                opt_str(file_ark),
                opt_str(source_key),
                opt_str(object_key),
                JsValue::from_bool(is_index),
            ],
        )
        .await?;
        Ok(())
    }

    async fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String> {
        call_cb(
            &self.callbacks,
            "deleteObject",
            &[JsValue::from_str(ns_id), JsValue::from_str(key)],
        )
        .await?;
        Ok(())
    }

    async fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &yaml::Value,
    ) -> Result<(), String> {
        let gates_json = to_json(gates, "sync_audience")?;
        call_cb(
            &self.callbacks,
            "syncAudience",
            &[
                JsValue::from_str(ns_id),
                JsValue::from_str(audience),
                JsValue::from_str(&gates_json),
            ],
        )
        .await?;
        Ok(())
    }

    async fn list_audiences(&self, ns_id: &str) -> Result<Vec<String>, String> {
        let result = call_cb(
            &self.callbacks,
            "listAudiences",
            &[JsValue::from_str(ns_id)],
        )
        .await?;
        let json = result
            .as_string()
            .ok_or_else(|| "listAudiences: expected a JSON string".to_string())?;
        let value = parse_json(&json, "listAudiences")?;
        <Vec<String> as fig::FromValue>::from_value(&value)
            .map_err(|e| format!("listAudiences decode: {e}"))
    }

    async fn delete_audience(&self, ns_id: &str, audience: &str) -> Result<(), String> {
        call_cb(
            &self.callbacks,
            "deleteAudience",
            &[JsValue::from_str(ns_id), JsValue::from_str(audience)],
        )
        .await?;
        Ok(())
    }

    async fn build_namespace(&self, ns_id: &str, base_url: Option<&str>) -> Result<(), String> {
        call_cb(
            &self.callbacks,
            "buildNamespace",
            &[JsValue::from_str(ns_id), opt_str(base_url)],
        )
        .await?;
        Ok(())
    }
}
