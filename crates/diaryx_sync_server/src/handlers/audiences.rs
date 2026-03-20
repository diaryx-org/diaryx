//! Audience visibility handlers — `PUT/GET/DELETE /namespaces/{id}/audiences/{name}`.

use crate::auth::RequireAuth;
use crate::tokens::create_signed_token;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, put},
};
use diaryx_server::domain::AudienceInfo;
use diaryx_server::ports::{BlobStore, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::audiences::AudienceService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Shared state for audience handlers.
#[derive(Clone)]
pub struct AudienceState {
    pub namespace_store: Arc<dyn NamespaceStore>,
    /// HMAC-SHA256 key used to sign audience access tokens.
    pub token_signing_key: Vec<u8>,
    /// Blob store for writing `_audiences.json` metadata to R2.
    pub blob_store: Arc<dyn BlobStore>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SetAudienceRequest {
    /// "public" | "token" | "private"
    pub access: String,
}

#[derive(Debug, Serialize)]
pub struct AudienceResponse {
    pub namespace_id: String,
    pub name: String,
    pub access: String,
}

impl From<AudienceInfo> for AudienceResponse {
    fn from(a: AudienceInfo) -> Self {
        Self {
            namespace_id: a.namespace_id,
            name: a.audience_name,
            access: a.access,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
}

// ---------------------------------------------------------------------------
// Router (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn audience_routes(state: AudienceState) -> Router {
    Router::new()
        .route("/audiences", get(list_audiences))
        .route(
            "/audiences/{name}",
            put(set_audience).delete(delete_audience),
        )
        .route("/audiences/{name}/token", get(get_audience_token))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_for_core_error(err: &ServerCoreError) -> StatusCode {
    match err {
        ServerCoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn core_error_response(err: ServerCoreError) -> axum::response::Response {
    let status = status_for_core_error(&err);
    (
        status,
        Json(serde_json::json!({ "error": err.to_string() })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// PUT /namespaces/{ns_id}/audiences/{name} — set access level for an audience.
async fn set_audience(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
    Json(req): Json<SetAudienceRequest>,
) -> impl IntoResponse {
    let service = AudienceService::new(state.namespace_store.as_ref(), state.blob_store.as_ref());

    match service.set(&ns_id, &name, &req.access, &auth.user.id).await {
        Ok(info) => Json(AudienceResponse::from(info)).into_response(),
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{ns_id}/audiences — list all audiences for a namespace.
async fn list_audiences(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    let service = AudienceService::new(state.namespace_store.as_ref(), state.blob_store.as_ref());

    match service.list(&ns_id, &auth.user.id).await {
        Ok(audiences) => {
            let response: Vec<AudienceResponse> =
                audiences.into_iter().map(AudienceResponse::from).collect();
            Json(response).into_response()
        }
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{ns_id}/audiences/{name}/token — generate a signed access token.
async fn get_audience_token(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let service = AudienceService::new(state.namespace_store.as_ref(), state.blob_store.as_ref());

    match service
        .require_token_eligible(&ns_id, &name, &auth.user.id)
        .await
    {
        Ok(_) => {
            let token_id = Uuid::new_v4().to_string();
            match create_signed_token(
                &state.token_signing_key,
                &ns_id,
                &name,
                &token_id,
                None, // no expiry
            ) {
                Ok(token) => Json(TokenResponse { token }).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(e) => core_error_response(e),
    }
}

/// DELETE /namespaces/{ns_id}/audiences/{name} — remove an audience record.
async fn delete_audience(
    State(state): State<AudienceState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let service = AudienceService::new(state.namespace_store.as_ref(), state.blob_store.as_ref());

    match service.delete(&ns_id, &name, &auth.user.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}
