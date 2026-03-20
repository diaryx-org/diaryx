//! Namespace session handlers — `POST/GET/PATCH/DELETE /sessions`.
//!
//! Sessions are a generic code → namespace mapping. Any namespace owner
//! can create a session; guests join via session code.

use crate::auth::RequireAuth;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use diaryx_server::ports::{NamespaceStore, ServerCoreError, SessionStore};
use diaryx_server::use_cases::sessions::SessionService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state for namespace session handlers.
#[derive(Clone)]
pub struct NsSessionState {
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub session_store: Arc<dyn SessionStore>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub namespace_id: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub read_only: bool,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub code: String,
    pub namespace_id: String,
    pub read_only: bool,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn ns_session_routes(state: NsSessionState) -> Router {
    Router::new()
        .route("/", post(create_session))
        .route(
            "/{code}",
            get(get_session)
                .patch(update_session)
                .delete(delete_session),
        )
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

/// POST /sessions — create a new session for a namespace.
async fn create_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let service = SessionService::new(state.namespace_store.as_ref(), state.session_store.as_ref());

    match service
        .create(&req.namespace_id, &auth.user.id, req.read_only)
        .await
    {
        Ok(session) => Json(SessionResponse {
            code: session.code,
            namespace_id: session.namespace_id,
            read_only: session.read_only,
        })
        .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// GET /sessions/{code} — get session info (unauthenticated, for guests to look up).
async fn get_session(
    State(state): State<NsSessionState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let service = SessionService::new(state.namespace_store.as_ref(), state.session_store.as_ref());

    match service.get(&code).await {
        Ok(session) => Json(SessionResponse {
            code: session.code,
            namespace_id: session.namespace_id,
            read_only: session.read_only,
        })
        .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// PATCH /sessions/{code} — update session settings (owner only).
async fn update_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let service = SessionService::new(state.namespace_store.as_ref(), state.session_store.as_ref());

    match service.update(&code, req.read_only, &auth.user.id).await {
        Ok(()) => Json(serde_json::json!({
            "code": code.to_uppercase(),
            "read_only": req.read_only,
        }))
        .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// DELETE /sessions/{code} — end a session (owner only).
async fn delete_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let service = SessionService::new(state.namespace_store.as_ref(), state.session_store.as_ref());

    match service.delete(&code, &auth.user.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}
