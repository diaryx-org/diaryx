//! Browser-side [`AuthenticatedClient`] for the web app.
//!
//! This module exposes a wasm-bindgen `AuthClient` class that wraps
//! [`diaryx_core::auth::AuthService`] with a [`WasmAuthenticatedClient`]
//! implementation. The client delegates all HTTP and credential persistence
//! back to JavaScript via a callbacks object so that `diaryx_core` and
//! `diaryx_wasm` stay free of any direct web-sys dependency on fetch, cookies,
//! or `localStorage`.
//!
//! ## Token lifecycle in the browser
//!
//! The browser never sees the raw session token. On successful verification
//! the sync server sets an `HttpOnly` cookie on the response, and every
//! subsequent fetch includes it via `credentials: 'include'`. The only state
//! held on the JS side is a boolean "has session" flag (mirrored in
//! `localStorage`) plus non-secret metadata (email, workspace id).
//!
//! ## JavaScript interface
//!
//! ```javascript
//! import { AuthClient } from './diaryx_wasm.js';
//!
//! const authClient = new AuthClient("https://app.diaryx.org/api", {
//!   fetch: async (method, path, body) => {
//!     const resp = await fetch(serverUrl + path, {
//!       method,
//!       headers: body ? { 'Content-Type': 'application/json' } : undefined,
//!       body,
//!       credentials: 'include',
//!     });
//!     return { status: resp.status, body: await resp.text() };
//!   },
//!   loadMetadata: async () => JSON.parse(localStorage.getItem('diaryx_auth_meta') ?? 'null'),
//!   saveMetadata: async (m) => localStorage.setItem('diaryx_auth_meta', JSON.stringify(m)),
//!   hasSession: async () => localStorage.getItem('diaryx_has_session') === 'true',
//!   storeSessionToken: async (_token) => localStorage.setItem('diaryx_has_session', 'true'),
//!   clearSession: async () => localStorage.removeItem('diaryx_has_session'),
//! });
//!
//! const response = await authClient.requestMagicLink('user@example.com');
//! ```

use diaryx_core::auth::{AuthError, AuthMetadata, AuthService, AuthenticatedClient, HttpResponse};
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// ============================================================================
// JS callbacks interface
// ============================================================================

#[wasm_bindgen]
extern "C" {
    /// JavaScript object providing HTTP + credential persistence callbacks.
    ///
    /// See the module-level docs for the expected shape. All callbacks are
    /// async (they may return plain values or Promises).
    #[wasm_bindgen(typescript_type = "AuthCallbacks")]
    pub type AuthCallbacks;
}

// ============================================================================
// Helpers
// ============================================================================

fn get_callback(callbacks: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

fn js_err_to_network(context: &str, err: JsValue) -> AuthError {
    let detail = err
        .as_string()
        .or_else(|| {
            Reflect::get(&err, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "unknown JS error".to_string());
    AuthError::network(format!("{context}: {detail}"))
}

async fn call_js_async(
    callbacks: &JsValue,
    name: &str,
    args: &[JsValue],
) -> Result<JsValue, AuthError> {
    let callback = get_callback(callbacks, name)
        .ok_or_else(|| AuthError::network(format!("auth callback '{name}' not provided")))?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => callback.call0(&this),
        1 => callback.call1(&this, &args[0]),
        2 => callback.call2(&this, &args[0], &args[1]),
        3 => callback.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let array = js_sys::Array::new();
            for arg in args {
                array.push(arg);
            }
            callback.apply(&this, &array)
        }
    }
    .map_err(|e| js_err_to_network(&format!("calling {name}"), e))?;

    if result.has_type::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise)
            .await
            .map_err(|e| js_err_to_network(&format!("awaiting {name}"), e))
    } else {
        Ok(result)
    }
}

fn js_to_http_response(value: JsValue, name: &str) -> Result<HttpResponse, AuthError> {
    let status = Reflect::get(&value, &JsValue::from_str("status"))
        .ok()
        .and_then(|v| v.as_f64())
        .map(|f| f as u16)
        .ok_or_else(|| {
            AuthError::network(format!("{name} callback returned no numeric 'status'"))
        })?;
    let body = Reflect::get(&value, &JsValue::from_str("body"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
    Ok(HttpResponse { status, body })
}

fn js_to_metadata(value: JsValue) -> Option<AuthMetadata> {
    if value.is_null() || value.is_undefined() {
        return None;
    }
    let email = Reflect::get(&value, &JsValue::from_str("email"))
        .ok()
        .and_then(|v| v.as_string());
    let workspace_id = Reflect::get(&value, &JsValue::from_str("workspace_id"))
        .ok()
        .and_then(|v| v.as_string())
        .or_else(|| {
            Reflect::get(&value, &JsValue::from_str("workspaceId"))
                .ok()
                .and_then(|v| v.as_string())
        });
    Some(AuthMetadata {
        email,
        workspace_id,
    })
}

fn metadata_to_js(metadata: &AuthMetadata) -> JsValue {
    let obj = js_sys::Object::new();
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("email"),
        &metadata
            .email
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
    );
    let _ = Reflect::set(
        &obj,
        &JsValue::from_str("workspace_id"),
        &metadata
            .workspace_id
            .as_deref()
            .map(JsValue::from_str)
            .unwrap_or(JsValue::NULL),
    );
    obj.into()
}

async fn http_call(
    callbacks: &JsValue,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<HttpResponse, AuthError> {
    let body_js = body.map(JsValue::from_str).unwrap_or(JsValue::NULL);
    let result = call_js_async(
        callbacks,
        "fetch",
        &[JsValue::from_str(method), JsValue::from_str(path), body_js],
    )
    .await?;
    js_to_http_response(result, "fetch")
}

// ============================================================================
// WasmAuthenticatedClient — implements the trait
// ============================================================================

/// Browser-side [`AuthenticatedClient`] that delegates HTTP and credential
/// persistence to JavaScript callbacks.
pub struct WasmAuthenticatedClient {
    server_url: String,
    callbacks: JsValue,
}

impl WasmAuthenticatedClient {
    pub fn new(server_url: String, callbacks: JsValue) -> Self {
        Self {
            server_url: server_url.trim_end_matches('/').to_string(),
            callbacks,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl AuthenticatedClient for WasmAuthenticatedClient {
    fn server_url(&self) -> &str {
        &self.server_url
    }

    async fn has_session(&self) -> bool {
        match call_js_async(&self.callbacks, "hasSession", &[]).await {
            Ok(v) => v.as_bool().unwrap_or(false),
            Err(_) => false,
        }
    }

    async fn load_metadata(&self) -> Option<AuthMetadata> {
        match call_js_async(&self.callbacks, "loadMetadata", &[]).await {
            Ok(v) => js_to_metadata(v),
            Err(_) => None,
        }
    }

    async fn save_metadata(&self, metadata: &AuthMetadata) {
        let meta_js = metadata_to_js(metadata);
        let _ = call_js_async(&self.callbacks, "saveMetadata", &[meta_js]).await;
    }

    async fn store_session_token(&self, token: &str) {
        let _ = call_js_async(
            &self.callbacks,
            "storeSessionToken",
            &[JsValue::from_str(token)],
        )
        .await;
    }

    async fn clear_session(&self) {
        let _ = call_js_async(&self.callbacks, "clearSession", &[]).await;
    }

    async fn get(&self, path: &str) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "GET", path, None).await
    }

    async fn post(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "POST", path, body).await
    }

    async fn put(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "PUT", path, body).await
    }

    async fn patch(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "PATCH", path, body).await
    }

    async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "DELETE", path, None).await
    }

    async fn get_unauth(&self, path: &str) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "GET", path, None).await
    }

    async fn post_unauth(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        http_call(&self.callbacks, "POST", path, body).await
    }
}

// ============================================================================
// AuthClient — wasm-bindgen public API
// ============================================================================

/// Auth client exposed to JavaScript.
///
/// Wraps [`AuthService`] with a [`WasmAuthenticatedClient`] backend. All
/// methods return JSON strings on success and throw a JavaScript `Error`
/// carrying `statusCode` and `devices` properties on failure.
#[wasm_bindgen]
pub struct AuthClient {
    inner: AuthService<WasmAuthenticatedClient>,
}

pub(crate) fn auth_error_to_js(err: AuthError) -> JsValue {
    let error = js_sys::Error::new(&err.message);
    let _ = Reflect::set(
        &error,
        &JsValue::from_str("statusCode"),
        &JsValue::from_f64(err.status_code as f64),
    );
    if let Some(devices) = &err.devices
        && let Ok(devices_str) = serde_json::to_string(devices)
        && let Ok(parsed) = js_sys::JSON::parse(&devices_str)
    {
        let _ = Reflect::set(&error, &JsValue::from_str("devices"), &parsed);
    }
    error.into()
}

pub(crate) fn to_js_ok<T: serde::Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_json::to_string(value)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| JsValue::from_str(&format!("serialize error: {e}")))
}

#[wasm_bindgen]
impl AuthClient {
    /// Create a new auth client targeting the given server URL.
    ///
    /// `callbacks` is a JavaScript object implementing the `AuthCallbacks`
    /// interface (see module docs for the expected shape).
    #[wasm_bindgen(constructor)]
    pub fn new(server_url: String, callbacks: AuthCallbacks) -> Self {
        let client = WasmAuthenticatedClient::new(server_url, callbacks.into());
        Self {
            inner: AuthService::new(client),
        }
    }

    /// Server URL this client targets.
    #[wasm_bindgen(js_name = serverUrl, getter)]
    pub fn server_url(&self) -> String {
        self.inner.server_url().to_string()
    }

    /// Whether a session is currently established (per the `hasSession` callback).
    #[wasm_bindgen(js_name = isAuthenticated)]
    pub async fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated().await
    }

    /// Load non-secret session metadata as a JSON string (or `null`).
    #[wasm_bindgen(js_name = getMetadata)]
    pub async fn get_metadata(&self) -> Result<JsValue, JsValue> {
        match self.inner.get_metadata().await {
            Some(m) => to_js_ok(&m),
            None => Ok(JsValue::NULL),
        }
    }

    // =========================================================================
    // Magic Link Flow
    // =========================================================================

    #[wasm_bindgen(js_name = requestMagicLink)]
    pub async fn request_magic_link(&self, email: String) -> Result<JsValue, JsValue> {
        self.inner
            .request_magic_link(&email)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = verifyMagicLink)]
    pub async fn verify_magic_link(
        &self,
        token: String,
        device_name: Option<String>,
        replace_device_id: Option<String>,
    ) -> Result<JsValue, JsValue> {
        self.inner
            .verify_magic_link(&token, device_name.as_deref(), replace_device_id.as_deref())
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = verifyCode)]
    pub async fn verify_code(
        &self,
        code: String,
        email: String,
        device_name: Option<String>,
        replace_device_id: Option<String>,
    ) -> Result<JsValue, JsValue> {
        self.inner
            .verify_code(
                &code,
                &email,
                device_name.as_deref(),
                replace_device_id.as_deref(),
            )
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    // =========================================================================
    // Session Management
    // =========================================================================

    #[wasm_bindgen(js_name = getMe)]
    pub async fn get_me(&self) -> Result<JsValue, JsValue> {
        self.inner
            .get_me()
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = refreshToken)]
    pub async fn refresh_token(&self) -> Result<JsValue, JsValue> {
        self.inner
            .refresh_token()
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen]
    pub async fn logout(&self) -> Result<(), JsValue> {
        self.inner.logout().await.map_err(auth_error_to_js)
    }

    // =========================================================================
    // Device Management
    // =========================================================================

    #[wasm_bindgen(js_name = getDevices)]
    pub async fn get_devices(&self) -> Result<JsValue, JsValue> {
        self.inner
            .get_devices()
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = renameDevice)]
    pub async fn rename_device(&self, device_id: String, new_name: String) -> Result<(), JsValue> {
        self.inner
            .rename_device(&device_id, &new_name)
            .await
            .map_err(auth_error_to_js)
    }

    #[wasm_bindgen(js_name = deleteDevice)]
    pub async fn delete_device(&self, device_id: String) -> Result<(), JsValue> {
        self.inner
            .delete_device(&device_id)
            .await
            .map_err(auth_error_to_js)
    }

    // =========================================================================
    // Account Management
    // =========================================================================

    #[wasm_bindgen(js_name = deleteAccount)]
    pub async fn delete_account(&self) -> Result<(), JsValue> {
        self.inner.delete_account().await.map_err(auth_error_to_js)
    }

    // =========================================================================
    // Workspace CRUD
    // =========================================================================

    #[wasm_bindgen(js_name = createWorkspace)]
    pub async fn create_workspace(&self, name: String) -> Result<JsValue, JsValue> {
        self.inner
            .create_workspace(&name)
            .await
            .map_err(auth_error_to_js)
            .and_then(|r| to_js_ok(&r))
    }

    #[wasm_bindgen(js_name = renameWorkspace)]
    pub async fn rename_workspace(
        &self,
        workspace_id: String,
        new_name: String,
    ) -> Result<(), JsValue> {
        self.inner
            .rename_workspace(&workspace_id, &new_name)
            .await
            .map_err(auth_error_to_js)
    }

    #[wasm_bindgen(js_name = deleteWorkspace)]
    pub async fn delete_workspace(&self, workspace_id: String) -> Result<(), JsValue> {
        self.inner
            .delete_workspace(&workspace_id)
            .await
            .map_err(auth_error_to_js)
    }
}
