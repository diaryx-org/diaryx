//! Tauri IPC commands that expose `diaryx_core::namespace` to the web layer.
//!
//! Follows the same shape as [`crate::auth_commands`]: each command reuses
//! the `AuthServiceState`'s `KeyringAuthenticatedClient` so the session
//! token never crosses IPC, and any `AuthError` is repackaged as the
//! familiar [`SerializableAuthError`] the frontend already knows how to
//! parse.

use diaryx_core::namespace;
use tauri::{AppHandle, State};

use crate::auth_commands::{AuthServiceState, SerializableAuthError, ensure_service};
use crate::fig_bridge::fig_to_json;

// ============================================================================
// Namespace CRUD
// ============================================================================

#[tauri::command]
pub async fn namespace_get(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::get_namespace(service.client(), &id)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_create(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: Option<String>,
    metadata: Option<serde_json::Value>,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let metadata = metadata.map(diaryx_core::yaml::Value::from);
    let result = namespace::create_namespace(service.client(), id.as_deref(), metadata.as_ref())
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_update_metadata(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    metadata: Option<serde_json::Value>,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let metadata = metadata.map(diaryx_core::yaml::Value::from);
    let result = namespace::update_namespace_metadata(service.client(), &id, metadata.as_ref())
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_delete(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    namespace::delete_namespace(service.client(), &id)
        .await
        .map_err(Into::into)
}

// ============================================================================
// Audiences
// ============================================================================

#[tauri::command]
pub async fn namespace_list_audiences(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::list_audiences(service.client(), &id)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_set_audience(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    name: String,
    access: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    namespace::set_audience(service.client(), &id, &name, &access)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn namespace_get_audience_token(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    name: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::get_audience_token(service.client(), &id, &name)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_rotate_audience_password(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    name: String,
    password: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::rotate_audience_password(service.client(), &id, &name, &password)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

// ============================================================================
// Subdomain
// ============================================================================

#[tauri::command]
pub async fn namespace_claim_subdomain(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    subdomain: String,
    default_audience: Option<String>,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::claim_subdomain(
        service.client(),
        &id,
        &subdomain,
        default_audience.as_deref(),
    )
    .await
    .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_release_subdomain(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    namespace::release_subdomain(service.client(), &id)
        .await
        .map_err(Into::into)
}

// ============================================================================
// Custom domains
// ============================================================================

#[tauri::command]
pub async fn namespace_list_domains(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::list_domains(service.client(), &id)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_register_domain(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    domain: String,
    audience_name: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::register_domain(service.client(), &id, &domain, &audience_name)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_remove_domain(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    domain: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    namespace::remove_domain(service.client(), &id, &domain)
        .await
        .map_err(Into::into)
}

// ============================================================================
// Subscribers
// ============================================================================

#[tauri::command]
pub async fn namespace_list_subscribers(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    audience: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::list_subscribers(service.client(), &id, &audience)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_add_subscriber(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    audience: String,
    email: String,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::add_subscriber(service.client(), &id, &audience, &email)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}

#[tauri::command]
pub async fn namespace_remove_subscriber(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    audience: String,
    contact_id: String,
) -> Result<(), SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    namespace::remove_subscriber(service.client(), &id, &audience, &contact_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn namespace_bulk_import_subscribers(
    state: State<'_, AuthServiceState>,
    app: AppHandle,
    id: String,
    audience: String,
    emails: Vec<String>,
) -> Result<serde_json::Value, SerializableAuthError> {
    let service = ensure_service(&state, &app).await?;
    let result = namespace::bulk_import_subscribers(service.client(), &id, &audience, &emails)
        .await
        .map_err(SerializableAuthError::from)?;
    Ok(fig_to_json(&result))
}
