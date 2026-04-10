//! Tauri IPC commands that expose `diaryx_core::auth::AuthService` to the
//! web layer.
//!
//! Each command is a thin async wrapper that:
//!
//! 1. Locks the `AuthServiceState`, returning a clear error if the app has
//!    not yet resolved an app data directory (which should be unreachable in
//!    production but keeps the type system honest).
//! 2. Calls the corresponding `AuthService` method.
//! 3. Converts any `AuthError` into a serializable form the JS side can
//!    inspect (with `statusCode` and optional `devices`).
//!
//! ## Token lifecycle (transitional)
//!
//! `KeyringAuthenticatedClient` persists the session token directly into the
//! OS keyring inside Rust. However, the web layer's legacy (non-migrated)
//! authService — used for passkeys, billing, attachments, and namespace
//! queries — still reads the token via `getTokenAsync()` → `credentials.rs`
//! and injects it as a `Bearer` header through `proxyFetch`. Until those
//! endpoints are migrated onto `AuthService` too, the verify commands return
//! the raw token so the web layer can mirror it into the legacy credential
//! store. Once the migration is complete, switch the return type to a
//! redacted form (see `RedactedVerifyResponse` below) so the token stops
//! crossing IPC entirely.

use std::path::PathBuf;
use std::sync::Arc;

use diaryx_core::auth::{
    AuthError, AuthMetadata, AuthService, Device, MagicLinkResponse, MeResponse, ServerWorkspace,
    User, VerifyResponse,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::RwLock;

use crate::auth_client::KeyringAuthenticatedClient;

// ============================================================================
// State
// ============================================================================

/// Shared auth service state managed by Tauri.
///
/// The inner `Option` is populated lazily from the app data directory on the
/// first command invocation (see [`ensure_service`]). This avoids forcing
/// `run()` to synchronously resolve the data dir at startup just to hand it
/// to the auth client.
pub struct AuthServiceState {
    inner: RwLock<Option<Arc<AuthService<KeyringAuthenticatedClient>>>>,
}

impl AuthServiceState {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl Default for AuthServiceState {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_data_dir(app: &AppHandle) -> Result<PathBuf, SerializableAuthError> {
    app.path()
        .app_data_dir()
        .map_err(|e| SerializableAuthError::local(format!("app_data_dir unavailable: {e}")))
}

async fn ensure_service(
    state: &State<'_, AuthServiceState>,
    app: &AppHandle,
) -> Result<Arc<AuthService<KeyringAuthenticatedClient>>, SerializableAuthError> {
    {
        let read = state.inner.read().await;
        if let Some(service) = read.as_ref() {
            return Ok(service.clone());
        }
    }

    let data_dir = resolve_data_dir(app)?;
    let client = KeyringAuthenticatedClient::from_app_data_dir(&data_dir, None);
    let service = Arc::new(AuthService::new(client));

    let mut write = state.inner.write().await;
    if let Some(existing) = write.as_ref() {
        return Ok(existing.clone());
    }
    *write = Some(service.clone());
    Ok(service)
}

async fn rebuild_service(
    state: &State<'_, AuthServiceState>,
    app: &AppHandle,
    server_override: Option<&str>,
) -> Result<Arc<AuthService<KeyringAuthenticatedClient>>, SerializableAuthError> {
    let data_dir = resolve_data_dir(app)?;
    let client = KeyringAuthenticatedClient::from_app_data_dir(&data_dir, server_override);
    let service = Arc::new(AuthService::new(client));

    let mut write = state.inner.write().await;
    *write = Some(service.clone());
    Ok(service)
}

// ============================================================================
// Serializable error surface
// ============================================================================

/// JSON-friendly projection of [`AuthError`] for the frontend.
///
/// Tauri serializes the `Err` variant of each command into JS via serde, so
/// this type is what the web layer sees — its fields line up with the ones
/// `wasmAuthService.ts` already expects on thrown errors.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializableAuthError {
    pub message: String,
    #[serde(rename = "statusCode")]
    pub status_code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<Device>>,
}

impl From<AuthError> for SerializableAuthError {
    fn from(err: AuthError) -> Self {
        Self {
            message: err.message,
            status_code: err.status_code,
            devices: err.devices,
        }
    }
}

impl SerializableAuthError {
    fn local(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            status_code: 0,
            devices: None,
        }
    }
}

// ============================================================================
// Redacted VerifyResponse — future-facing; currently unused.
// ============================================================================

/// A [`VerifyResponse`] with the token field cleared before crossing IPC.
///
/// Wired up for the eventual end state (token stays in Rust), but not used
/// yet — see the module-level note on token lifecycle. The migrated verify
/// commands still return the full `VerifyResponse` so the legacy web-side
/// token-store pathway keeps working during the transition.
#[allow(dead_code)]
#[derive(Debug, Serialize)]
pub struct RedactedVerifyResponse {
    pub success: bool,
    /// Always an empty string once redaction is turned on.
    pub token: String,
    pub user: User,
}

impl From<VerifyResponse> for RedactedVerifyResponse {
    fn from(v: VerifyResponse) -> Self {
        Self {
            success: v.success,
            token: String::new(),
            user: v.user,
        }
    }
}

// ============================================================================
// Server URL management (not part of AuthService, but needed by the UI).
// ============================================================================

#[tauri::command]
pub async fn auth_server_url(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<String, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    Ok(service.server_url().to_string())
}

#[tauri::command]
pub async fn auth_set_server_url(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    server_url: String,
) -> Result<(), SerializableAuthError> {
    rebuild_service(&state, &app, Some(&server_url)).await?;
    Ok(())
}

#[tauri::command]
pub async fn auth_is_authenticated(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<bool, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    Ok(service.is_authenticated().await)
}

#[tauri::command]
pub async fn auth_get_metadata(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<Option<AuthMetadata>, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    Ok(service.get_metadata().await)
}

// ============================================================================
// Magic link flow
// ============================================================================

#[tauri::command]
pub async fn auth_request_magic_link(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    email: String,
) -> Result<MagicLinkResponse, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.request_magic_link(&email).await.map_err(Into::into)
}

#[tauri::command]
pub async fn auth_verify_magic_link(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    token: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
) -> Result<VerifyResponse, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service
        .verify_magic_link(&token, device_name.as_deref(), replace_device_id.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn auth_verify_code(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    code: String,
    email: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
) -> Result<VerifyResponse, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service
        .verify_code(
            &code,
            &email,
            device_name.as_deref(),
            replace_device_id.as_deref(),
        )
        .await
        .map_err(Into::into)
}

// ============================================================================
// Session management
// ============================================================================

#[tauri::command]
pub async fn auth_get_me(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<MeResponse, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.get_me().await.map_err(Into::into)
}

#[tauri::command]
pub async fn auth_refresh_token(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<MeResponse, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.refresh_token().await.map_err(Into::into)
}

#[tauri::command]
pub async fn auth_logout(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.logout().await.map_err(Into::into)
}

// ============================================================================
// Device management
// ============================================================================

#[tauri::command]
pub async fn auth_get_devices(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<Vec<Device>, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.get_devices().await.map_err(Into::into)
}

#[tauri::command]
pub async fn auth_rename_device(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    device_id: String,
    new_name: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service
        .rename_device(&device_id, &new_name)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn auth_delete_device(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    device_id: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.delete_device(&device_id).await.map_err(Into::into)
}

// ============================================================================
// Account management
// ============================================================================

#[tauri::command]
pub async fn auth_delete_account(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.delete_account().await.map_err(Into::into)
}

// ============================================================================
// Workspace CRUD
// ============================================================================

#[tauri::command]
pub async fn auth_create_workspace(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    name: String,
) -> Result<ServerWorkspace, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service.create_workspace(&name).await.map_err(Into::into)
}

#[tauri::command]
pub async fn auth_rename_workspace(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    workspace_id: String,
    new_name: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service
        .rename_workspace(&workspace_id, &new_name)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn auth_delete_workspace(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    workspace_id: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    service
        .delete_workspace(&workspace_id)
        .await
        .map_err(Into::into)
}
