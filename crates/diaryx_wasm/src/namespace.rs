//! Browser-side `NamespaceClient` exposing [`diaryx_core::namespace`] to
//! JavaScript.
//!
//! This mirrors [`crate::auth::AuthClient`]: it wraps
//! [`crate::auth::WasmAuthenticatedClient`] — reusing the same
//! `AuthCallbacks` shape (`fetch(method, path, body)` is the only field the
//! namespace functions actually exercise) — and exposes each free function
//! from `diaryx_core::namespace` as a wasm-bindgen method that returns a
//! JSON-string promise on success or an Error carrying `statusCode` on
//! failure.
//!
//! ## JavaScript interface
//!
//! ```javascript
//! import { NamespaceClient } from './diaryx_wasm.js';
//!
//! const ns = new NamespaceClient("https://app.diaryx.org/api", {
//!   fetch: async (method, path, body) => {
//!     const resp = await fetch(serverUrl + path, {
//!       method,
//!       headers: body ? { 'Content-Type': 'application/json' } : undefined,
//!       body,
//!       credentials: 'include',
//!     });
//!     return { status: resp.status, body: await resp.text() };
//!   },
//!   // The rest of the AuthCallbacks fields are unused by NamespaceClient,
//!   // but the trait still requires them; pass no-ops.
//!   loadMetadata: async () => null,
//!   saveMetadata: async () => {},
//!   hasSession: async () => true,
//!   storeSessionToken: async () => {},
//!   clearSession: async () => {},
//! });
//!
//! const meta = await ns.createNamespace("ns-123", { name: "My Journal" });
//! ```

use diaryx_core::namespace;
use wasm_bindgen::prelude::*;

use crate::auth::{AuthCallbacks, WasmAuthenticatedClient, auth_error_to_js, to_js_ok};

/// Namespace client exposed to JavaScript.
///
/// Wraps [`diaryx_core::namespace`] free functions with a
/// [`WasmAuthenticatedClient`] backend. All methods return JSON strings on
/// success and throw a JavaScript `Error` carrying a `statusCode` property
/// on failure (mirroring [`crate::auth::AuthClient`]).
#[wasm_bindgen]
pub struct NamespaceClient {
    client: WasmAuthenticatedClient,
}

#[wasm_bindgen]
impl NamespaceClient {
    /// Create a new namespace client targeting the given server URL.
    ///
    /// `callbacks` is the same `AuthCallbacks` interface [`AuthClient`] uses
    /// (see module docs) — only the `fetch` callback is actually invoked by
    /// namespace operations.
    #[wasm_bindgen(constructor)]
    pub fn new(server_url: String, callbacks: AuthCallbacks) -> Self {
        let client = WasmAuthenticatedClient::new(server_url, callbacks.into());
        Self { client }
    }

    /// Server URL this client targets.
    #[wasm_bindgen(js_name = serverUrl, getter)]
    pub fn server_url(&self) -> String {
        diaryx_core::auth::AuthenticatedClient::server_url(&self.client).to_string()
    }

    // =========================================================================
    // Namespace CRUD
    // =========================================================================

    /// Fetch metadata for a single namespace.
    #[wasm_bindgen(js_name = getNamespace)]
    pub async fn get_namespace(&self, id: String) -> Result<JsValue, JsValue> {
        namespace::get_namespace(&self.client, &id)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    /// Create a new namespace. `id` is optional (server-assigned when null);
    /// `metadata` is an arbitrary JSON value stored verbatim.
    #[wasm_bindgen(js_name = createNamespace)]
    pub async fn create_namespace(
        &self,
        id: Option<String>,
        metadata: JsValue,
    ) -> Result<JsValue, JsValue> {
        let metadata_value = js_to_optional_json(&metadata)?;
        namespace::create_namespace(&self.client, id.as_deref(), metadata_value.as_ref())
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    /// Replace the `metadata` blob on an existing namespace.
    #[wasm_bindgen(js_name = updateNamespaceMetadata)]
    pub async fn update_namespace_metadata(
        &self,
        id: String,
        metadata: JsValue,
    ) -> Result<JsValue, JsValue> {
        let metadata_value = js_to_optional_json(&metadata)?;
        namespace::update_namespace_metadata(&self.client, &id, metadata_value.as_ref())
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    /// Delete a namespace on the server.
    ///
    /// Treats 404 as an idempotent success (see
    /// [`diaryx_core::namespace::delete_namespace`]).
    #[wasm_bindgen(js_name = deleteNamespace)]
    pub async fn delete_namespace(&self, id: String) -> Result<(), JsValue> {
        namespace::delete_namespace(&self.client, &id)
            .await
            .map_err(auth_error_to_js)
    }

    // =========================================================================
    // Audiences
    // =========================================================================

    #[wasm_bindgen(js_name = listAudiences)]
    pub async fn list_audiences(&self, id: String) -> Result<JsValue, JsValue> {
        namespace::list_audiences(&self.client, &id)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = setAudience)]
    pub async fn set_audience(
        &self,
        id: String,
        name: String,
        access: String,
    ) -> Result<(), JsValue> {
        namespace::set_audience(&self.client, &id, &name, &access)
            .await
            .map_err(auth_error_to_js)
    }

    #[wasm_bindgen(js_name = getAudienceToken)]
    pub async fn get_audience_token(&self, id: String, name: String) -> Result<JsValue, JsValue> {
        namespace::get_audience_token(&self.client, &id, &name)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    // =========================================================================
    // Subdomain
    // =========================================================================

    #[wasm_bindgen(js_name = claimSubdomain)]
    pub async fn claim_subdomain(
        &self,
        id: String,
        subdomain: String,
        default_audience: Option<String>,
    ) -> Result<JsValue, JsValue> {
        namespace::claim_subdomain(&self.client, &id, &subdomain, default_audience.as_deref())
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = releaseSubdomain)]
    pub async fn release_subdomain(&self, id: String) -> Result<(), JsValue> {
        namespace::release_subdomain(&self.client, &id)
            .await
            .map_err(auth_error_to_js)
    }

    // =========================================================================
    // Custom domains
    // =========================================================================

    #[wasm_bindgen(js_name = listDomains)]
    pub async fn list_domains(&self, id: String) -> Result<JsValue, JsValue> {
        namespace::list_domains(&self.client, &id)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = registerDomain)]
    pub async fn register_domain(
        &self,
        id: String,
        domain: String,
        audience_name: String,
    ) -> Result<JsValue, JsValue> {
        namespace::register_domain(&self.client, &id, &domain, &audience_name)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = removeDomain)]
    pub async fn remove_domain(&self, id: String, domain: String) -> Result<(), JsValue> {
        namespace::remove_domain(&self.client, &id, &domain)
            .await
            .map_err(auth_error_to_js)
    }

    // =========================================================================
    // Subscribers
    // =========================================================================

    #[wasm_bindgen(js_name = listSubscribers)]
    pub async fn list_subscribers(&self, id: String, audience: String) -> Result<JsValue, JsValue> {
        namespace::list_subscribers(&self.client, &id, &audience)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = addSubscriber)]
    pub async fn add_subscriber(
        &self,
        id: String,
        audience: String,
        email: String,
    ) -> Result<JsValue, JsValue> {
        namespace::add_subscriber(&self.client, &id, &audience, &email)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = removeSubscriber)]
    pub async fn remove_subscriber(
        &self,
        id: String,
        audience: String,
        contact_id: String,
    ) -> Result<(), JsValue> {
        namespace::remove_subscriber(&self.client, &id, &audience, &contact_id)
            .await
            .map_err(auth_error_to_js)
    }

    #[wasm_bindgen(js_name = bulkImportSubscribers)]
    pub async fn bulk_import_subscribers(
        &self,
        id: String,
        audience: String,
        emails: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        namespace::bulk_import_subscribers(&self.client, &id, &audience, &emails)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }
}

/// Convert a `JsValue` to `Option<serde_json::Value>`, treating `null` /
/// `undefined` as `None` and other values as JSON via `JSON.stringify` +
/// parse.
///
/// This avoids pulling in `serde-wasm-bindgen` just to decode a value that
/// the caller will re-encode immediately inside `create_namespace` /
/// `update_namespace_metadata`.
fn js_to_optional_json(value: &JsValue) -> Result<Option<serde_json::Value>, JsValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let stringified = js_sys::JSON::stringify(value)
        .map_err(|e| JsValue::from_str(&format!("metadata not JSON-serializable: {e:?}")))?;
    let as_rust_string = stringified
        .as_string()
        .ok_or_else(|| JsValue::from_str("JSON.stringify did not return a string"))?;
    let parsed: serde_json::Value = serde_json::from_str(&as_rust_string)
        .map_err(|e| JsValue::from_str(&format!("invalid metadata JSON: {e}")))?;
    if parsed.is_null() {
        Ok(None)
    } else {
        Ok(Some(parsed))
    }
}
