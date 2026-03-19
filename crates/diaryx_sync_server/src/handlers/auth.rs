use crate::auth::{MagicLinkService, PasskeyService, RequireAuth};
use crate::db::{AuthRepo, NamespaceRepo};
use crate::email::EmailService;
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
};
use diaryx_server::ports::{AuthStore, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::current_user::CurrentUserService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Shared state for auth handlers
#[derive(Clone)]
pub struct AuthState {
    pub magic_link_service: Arc<MagicLinkService>,
    pub email_service: Arc<EmailService>,
    pub repo: Arc<AuthRepo>,
    pub ns_repo: Arc<NamespaceRepo>,
    pub auth_store: Arc<dyn AuthStore>,
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub passkey_service: Arc<PasskeyService>,
    /// Session expiry in days, used for cookie Max-Age.
    pub session_expiry_days: i64,
    /// Whether to set the `Secure` flag on session cookies.
    pub secure_cookies: bool,
}

/// Request body for magic link request
#[derive(Debug, Deserialize)]
pub struct MagicLinkRequest {
    pub email: String,
}

/// Response for magic link request
#[derive(Debug, Serialize)]
pub struct MagicLinkResponse {
    pub success: bool,
    pub message: String,
    /// Only included in dev mode when email is not configured
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_link: Option<String>,
    /// Only included in dev mode when email is not configured
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dev_code: Option<String>,
}

/// Request body for verification code
#[derive(Debug, Deserialize)]
pub struct VerifyCodeRequest {
    pub code: String,
    pub email: String,
    pub device_name: Option<String>,
    /// When the device limit has been reached, the client can re-submit the
    /// request with this field set to the ID of the device to replace.
    pub replace_device_id: Option<String>,
}

/// Query params for magic link verification
#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    pub token: String,
    pub device_name: Option<String>,
    pub replace_device_id: Option<String>,
}

/// Response for successful verification
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub success: bool,
    pub token: String,
    pub user: UserResponse,
}

/// User info in responses
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    /// When the error is "device_limit_reached", this contains the user's
    /// existing devices so the client can offer a replacement prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<crate::auth::DeviceLimitDevice>>,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            devices: None,
        }
    }
}

fn status_for_core_error(error: &ServerCoreError) -> StatusCode {
    match error {
        ServerCoreError::InvalidInput(_) | ServerCoreError::Conflict(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Response for user info
#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: UserResponse,
    pub workspaces: Vec<WorkspaceResponse>,
    pub devices: Vec<DeviceResponse>,
    pub tier: String,
    pub workspace_limit: u32,
    pub published_site_limit: u32,
    pub attachment_limit_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub id: String,
    pub name: Option<String>,
    pub last_seen_at: String,
}

/// Session cookie name.
const SESSION_COOKIE: &str = "diaryx_session";

/// Build a `Set-Cookie` header that sets the session cookie.
fn set_session_cookie(token: &str, max_age_days: i64, secure: bool) -> String {
    let max_age_secs = max_age_days * 86400;
    if secure {
        format!(
            "{SESSION_COOKIE}={token}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
        )
    } else {
        format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}")
    }
}

/// Build a `Set-Cookie` header that clears the session cookie.
fn clear_session_cookie(secure: bool) -> String {
    if secure {
        format!("{SESSION_COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
    } else {
        format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
    }
}

/// Create auth routes
pub fn auth_routes(state: AuthState) -> Router {
    Router::new()
        .route("/magic-link", post(request_magic_link))
        .route("/verify", get(verify_magic_link))
        .route("/verify-code", post(verify_code))
        .route("/me", get(get_current_user))
        .route("/logout", post(logout))
        .route("/account", delete(delete_account))
        .route("/devices", get(list_devices))
        .route(
            "/devices/{device_id}",
            axum::routing::patch(rename_device).delete(delete_device),
        )
        // Passkey routes
        .route("/passkeys/register/start", post(passkey_register_start))
        .route("/passkeys/register/finish", post(passkey_register_finish))
        .route("/passkeys/authenticate/start", post(passkey_auth_start))
        .route("/passkeys/authenticate/finish", post(passkey_auth_finish))
        .route("/passkeys", get(passkey_list))
        .route("/passkeys/{id}", delete(passkey_delete))
        .with_state(state)
}

/// POST /auth/magic-link - Request a magic link
async fn request_magic_link(
    State(state): State<AuthState>,
    Json(body): Json<MagicLinkRequest>,
) -> impl IntoResponse {
    let email = body.email.trim().to_lowercase();

    // Validate email format
    if !email.contains('@') || email.len() < 5 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Invalid email address")),
        )
            .into_response();
    }

    // Request magic link
    let (token, code) = match state.magic_link_service.request_magic_link(&email) {
        Ok(result) => result,
        Err(crate::auth::MagicLinkError::RateLimited) => {
            warn!("Rate limited magic link request for {}", email);
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorResponse::new(
                    "Too many requests. Please try again later.",
                )),
            )
                .into_response();
        }
        Err(e) => {
            error!("Failed to create magic link: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to create magic link")),
            )
                .into_response();
        }
    };

    let magic_link_url = state.magic_link_service.build_magic_link_url(&token);

    // Try to send email
    if state.email_service.is_configured() {
        if let Err(e) = state
            .email_service
            .send_magic_link(&email, &magic_link_url, &code)
            .await
        {
            error!("Failed to send magic link email: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to send email")),
            )
                .into_response();
        }

        info!("Magic link sent to {}", email);
        (
            StatusCode::OK,
            Json(MagicLinkResponse {
                success: true,
                message: "Check your email for a sign-in link.".to_string(),
                dev_link: None,
                dev_code: None,
            }),
        )
            .into_response()
    } else {
        // Dev mode: return the link and code directly
        warn!(
            "Email not configured, returning magic link directly (dev mode only!): {}",
            magic_link_url
        );
        (
            StatusCode::OK,
            Json(MagicLinkResponse {
                success: true,
                message: "Email not configured. Use the dev link below.".to_string(),
                dev_link: Some(magic_link_url),
                dev_code: Some(code),
            }),
        )
            .into_response()
    }
}

/// GET /auth/verify - Verify a magic link and return session token
async fn verify_magic_link(
    State(state): State<AuthState>,
    Query(query): Query<VerifyQuery>,
) -> impl IntoResponse {
    let result = state.magic_link_service.verify_magic_link(
        &query.token,
        query.device_name.as_deref(),
        None, // Could extract user-agent from headers
        query.replace_device_id.as_deref(),
    );

    match result {
        Ok(verify_result) => {
            info!("User {} logged in successfully", verify_result.email);
            let mut headers = HeaderMap::new();
            if let Ok(cookie) = set_session_cookie(
                &verify_result.session_token,
                state.session_expiry_days,
                state.secure_cookies,
            )
            .parse()
            {
                headers.insert(header::SET_COOKIE, cookie);
            }
            (
                StatusCode::OK,
                headers,
                Json(VerifyResponse {
                    success: true,
                    token: verify_result.session_token,
                    user: UserResponse {
                        id: verify_result.user_id,
                        email: verify_result.email,
                    },
                }),
            )
                .into_response()
        }
        Err(crate::auth::MagicLinkError::InvalidToken) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Invalid or expired link. Please request a new one.",
            )),
        )
            .into_response(),
        Err(crate::auth::MagicLinkError::InvalidReplaceDevice) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "The device to replace was not found on this account.",
            )),
        )
            .into_response(),
        Err(crate::auth::MagicLinkError::DeviceLimitReached { devices, .. }) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Device limit reached. Remove a device to sign in on a new one.".to_string(),
                devices: Some(devices),
            }),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to verify magic link: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Verification failed")),
            )
                .into_response()
        }
    }
}

/// POST /auth/verify-code - Verify a 6-digit code and return session token
async fn verify_code(
    State(state): State<AuthState>,
    Json(body): Json<VerifyCodeRequest>,
) -> impl IntoResponse {
    let result = state.magic_link_service.verify_code(
        &body.code,
        &body.email,
        body.device_name.as_deref(),
        None,
        body.replace_device_id.as_deref(),
    );

    match result {
        Ok(verify_result) => {
            info!(
                "User {} logged in via verification code",
                verify_result.email
            );
            let mut headers = HeaderMap::new();
            if let Ok(cookie) = set_session_cookie(
                &verify_result.session_token,
                state.session_expiry_days,
                state.secure_cookies,
            )
            .parse()
            {
                headers.insert(header::SET_COOKIE, cookie);
            }
            (
                StatusCode::OK,
                headers,
                Json(VerifyResponse {
                    success: true,
                    token: verify_result.session_token,
                    user: UserResponse {
                        id: verify_result.user_id,
                        email: verify_result.email,
                    },
                }),
            )
                .into_response()
        }
        Err(crate::auth::MagicLinkError::InvalidToken) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "Invalid or expired code. Please request a new one.",
            )),
        )
            .into_response(),
        Err(crate::auth::MagicLinkError::InvalidReplaceDevice) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "The device to replace was not found on this account.",
            )),
        )
            .into_response(),
        Err(crate::auth::MagicLinkError::DeviceLimitReached { devices, .. }) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Device limit reached. Remove a device to sign in on a new one.".to_string(),
                devices: Some(devices),
            }),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to verify code: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Verification failed")),
            )
                .into_response()
        }
    }
}

/// GET /auth/me - Get current user info
async fn get_current_user(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let service =
        CurrentUserService::new(state.auth_store.as_ref(), state.namespace_store.as_ref());
    let context = match service.load(&auth.user.id, &auth.user.email).await {
        Ok(context) => context,
        Err(err) => {
            error!("Failed to load current user context: {}", err);
            return (
                status_for_core_error(&err),
                Json(ErrorResponse::new(err.to_string())),
            )
                .into_response();
        }
    };

    let devices = context
        .devices
        .into_iter()
        .map(|device| DeviceResponse {
            id: device.id,
            name: device.name,
            last_seen_at: device.last_seen_at.to_rfc3339(),
        })
        .collect::<Vec<_>>();

    let workspaces = context
        .namespaces
        .into_iter()
        .map(|ns| WorkspaceResponse {
            id: ns.id.clone(),
            name: ns.id,
        })
        .collect::<Vec<_>>();

    Json(MeResponse {
        user: UserResponse {
            id: auth.user.id,
            email: context.user.email,
        },
        workspaces,
        devices,
        tier: context.user.tier.as_str().to_string(),
        workspace_limit: context.limits.workspace_limit,
        published_site_limit: context.limits.published_site_limit,
        attachment_limit_bytes: context.limits.attachment_limit_bytes,
    })
    .into_response()
}

/// POST /auth/logout - Log out (delete session)
async fn logout(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    if let Err(e) = state.repo.delete_session(&auth.session.token) {
        error!("Failed to delete session: {}", e);
    }

    let mut headers = HeaderMap::new();
    if let Ok(cookie) = clear_session_cookie(state.secure_cookies).parse() {
        headers.insert(header::SET_COOKIE, cookie);
    }

    (StatusCode::NO_CONTENT, headers)
}

/// GET /auth/devices - List user's devices
async fn list_devices(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let devices = state
        .auth_store
        .list_user_devices(&auth.user.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| DeviceResponse {
            id: d.id,
            name: d.name,
            last_seen_at: d.last_seen_at.to_rfc3339(),
        })
        .collect::<Vec<_>>();

    Json(devices)
}

/// Request body for device rename
#[derive(Debug, Deserialize)]
pub struct RenameDeviceRequest {
    pub name: String,
}

/// PATCH /auth/devices/:device_id - Rename a device
async fn rename_device(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(device_id): axum::extract::Path<String>,
    Json(body): Json<RenameDeviceRequest>,
) -> impl IntoResponse {
    // Verify the device belongs to the user
    let devices = state
        .auth_store
        .list_user_devices(&auth.user.id)
        .await
        .unwrap_or_default();

    if !devices.iter().any(|d| d.id == device_id) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let name = body.name.trim().to_string();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Device name cannot be empty")),
        )
            .into_response();
    }

    match state.auth_store.rename_device(&device_id, &name).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            error!("Failed to rename device: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE /auth/devices/:device_id - Delete a device
async fn delete_device(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(device_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Verify the device belongs to the user
    let devices = state
        .auth_store
        .list_user_devices(&auth.user.id)
        .await
        .unwrap_or_default();

    let owns_device = devices.iter().any(|d| d.id == device_id);

    if !owns_device {
        return StatusCode::NOT_FOUND;
    }

    // Don't allow deleting the current device
    if device_id == auth.session.device_id {
        return StatusCode::BAD_REQUEST;
    }

    if let Err(e) = state.auth_store.delete_device(&device_id).await {
        error!("Failed to delete device: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR;
    }

    StatusCode::NO_CONTENT
}

/// DELETE /auth/account - Delete user account and all server data
async fn delete_account(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let user_id = &auth.user.id;

    info!("Deleting account for user: {}", user_id);

    // Delete user from database (CASCADE deletes namespaces, sessions, etc.)
    match state.repo.delete_user(user_id) {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to delete user {}: {}", user_id, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to delete account")),
            )
                .into_response();
        }
    };

    info!("Successfully deleted account for user: {}", user_id);

    StatusCode::NO_CONTENT.into_response()
}

// ===== Passkey handlers =====

#[derive(Debug, Serialize)]
struct PasskeyRegisterStartResponse {
    challenge_id: String,
    options: serde_json::Value,
}

/// POST /auth/passkeys/register/start (RequireAuth)
async fn passkey_register_start(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    match state
        .passkey_service
        .start_registration(&auth.user.id, &auth.user.email)
    {
        Ok((ccr, challenge_id)) => {
            let options = serde_json::to_value(&ccr).unwrap_or_default();
            (
                StatusCode::OK,
                Json(PasskeyRegisterStartResponse {
                    challenge_id,
                    options,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Passkey register start failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct PasskeyRegisterFinishRequest {
    challenge_id: String,
    name: String,
    credential: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct PasskeyRegisterFinishResponse {
    id: String,
}

/// POST /auth/passkeys/register/finish (RequireAuth)
async fn passkey_register_finish(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
    Json(body): Json<PasskeyRegisterFinishRequest>,
) -> impl IntoResponse {
    let credential = match serde_json::from_value(body.credential) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(format!("Invalid credential: {}", e))),
            )
                .into_response();
        }
    };

    match state.passkey_service.finish_registration(
        &body.challenge_id,
        &auth.user.id,
        &body.name,
        &credential,
    ) {
        Ok(id) => {
            info!(
                "Passkey '{}' registered for user {}",
                body.name, auth.user.email
            );
            (StatusCode::OK, Json(PasskeyRegisterFinishResponse { id })).into_response()
        }
        Err(crate::auth::PasskeyError::ChallengeNotFound) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Challenge expired or not found")),
        )
            .into_response(),
        Err(e) => {
            error!("Passkey register finish failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct PasskeyAuthStartRequest {
    email: Option<String>,
}

#[derive(Debug, Serialize)]
struct PasskeyAuthStartResponse {
    challenge_id: String,
    options: serde_json::Value,
}

/// POST /auth/passkeys/authenticate/start (public)
async fn passkey_auth_start(
    State(state): State<AuthState>,
    Json(body): Json<PasskeyAuthStartRequest>,
) -> impl IntoResponse {
    let result = match body
        .email
        .as_deref()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
    {
        Some(email) => {
            let email = email.to_lowercase();
            state.passkey_service.start_authentication(&email)
        }
        None => state.passkey_service.start_discoverable_authentication(),
    };

    match result {
        Ok((rcr, challenge_id)) => {
            let options = serde_json::to_value(&rcr).unwrap_or_default();
            (
                StatusCode::OK,
                Json(PasskeyAuthStartResponse {
                    challenge_id,
                    options,
                }),
            )
                .into_response()
        }
        Err(crate::auth::PasskeyError::NoPasskeys) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("No passkeys registered for this email")),
        )
            .into_response(),
        Err(e) => {
            error!("Passkey auth start failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct PasskeyAuthFinishRequest {
    challenge_id: String,
    credential: serde_json::Value,
    device_name: Option<String>,
    replace_device_id: Option<String>,
}

/// POST /auth/passkeys/authenticate/finish (public) → VerifyResponse
async fn passkey_auth_finish(
    State(state): State<AuthState>,
    Json(body): Json<PasskeyAuthFinishRequest>,
) -> impl IntoResponse {
    let credential = match serde_json::from_value(body.credential) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(format!("Invalid credential: {}", e))),
            )
                .into_response();
        }
    };

    match state.passkey_service.finish_any_authentication(
        &body.challenge_id,
        &credential,
        body.device_name.as_deref(),
        None,
        body.replace_device_id.as_deref(),
    ) {
        Ok(result) => {
            info!("User {} logged in via passkey", result.email);
            let mut headers = HeaderMap::new();
            if let Ok(cookie) = set_session_cookie(
                &result.session_token,
                state.session_expiry_days,
                state.secure_cookies,
            )
            .parse()
            {
                headers.insert(header::SET_COOKIE, cookie);
            }
            (
                StatusCode::OK,
                headers,
                Json(VerifyResponse {
                    success: true,
                    token: result.session_token,
                    user: UserResponse {
                        id: result.user_id,
                        email: result.email,
                    },
                }),
            )
                .into_response()
        }
        Err(crate::auth::PasskeyError::ChallengeNotFound) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Challenge expired or not found")),
        )
            .into_response(),
        Err(crate::auth::PasskeyError::InvalidCredential(msg)) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Authentication failed: {}",
                msg
            ))),
        )
            .into_response(),
        Err(crate::auth::PasskeyError::DeviceLimitReached { devices, .. }) => (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Device limit reached. Remove a device to sign in on a new one.".to_string(),
                devices: Some(devices),
            }),
        )
            .into_response(),
        Err(e) => {
            error!("Passkey auth finish failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Serialize)]
struct PasskeyListItem {
    id: String,
    name: String,
    created_at: i64,
    last_used_at: Option<i64>,
}

/// GET /auth/passkeys (RequireAuth) — list user's passkeys
async fn passkey_list(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    match state.passkey_service.list_passkeys(&auth.user.id) {
        Ok(passkeys) => {
            let items: Vec<PasskeyListItem> = passkeys
                .into_iter()
                .map(|p| PasskeyListItem {
                    id: p.id,
                    name: p.name,
                    created_at: p.created_at,
                    last_used_at: p.last_used_at,
                })
                .collect();
            Json(items).into_response()
        }
        Err(e) => {
            error!("Failed to list passkeys: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to list passkeys")),
            )
                .into_response()
        }
    }
}

/// DELETE /auth/passkeys/:id (RequireAuth)
async fn passkey_delete(
    State(state): State<AuthState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.passkey_service.delete_passkey(&id, &auth.user.id) {
        Ok(true) => {
            info!("Passkey {} deleted for user {}", id, auth.user.email);
            StatusCode::NO_CONTENT
        }
        Ok(false) => StatusCode::NOT_FOUND,
        Err(e) => {
            error!("Failed to delete passkey: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
