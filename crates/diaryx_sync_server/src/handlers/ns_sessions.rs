//! Namespace session handlers — `POST/GET/PATCH/DELETE /sessions`.
//!
//! Sessions are a generic code → namespace mapping. Any namespace owner
//! can create a session; guests join via session code.

use crate::auth::RequireAuth;
use crate::db::NamespaceRepo;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state for namespace session handlers.
#[derive(Clone)]
pub struct NsSessionState {
    pub ns_repo: Arc<NamespaceRepo>,
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
// Handlers
// ---------------------------------------------------------------------------

/// POST /sessions — create a new session for a namespace.
async fn create_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    // Verify caller owns the namespace
    match state.ns_repo.get_namespace(&req.namespace_id) {
        Some(ns) if ns.owner_user_id == auth.user.id => {}
        Some(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": "You do not own this namespace" })),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Namespace not found" })),
            )
                .into_response();
        }
    }

    match state
        .ns_repo
        .create_session(&req.namespace_id, &auth.user.id, req.read_only, None)
    {
        Ok(code) => Json(SessionResponse {
            code,
            namespace_id: req.namespace_id,
            read_only: req.read_only,
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /sessions/{code} — get session info (unauthenticated, for guests to look up).
async fn get_session(
    State(state): State<NsSessionState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let code = code.to_uppercase();
    match state.ns_repo.get_session(&code) {
        Some(session) => Json(SessionResponse {
            code: session.code,
            namespace_id: session.namespace_id,
            read_only: session.read_only,
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found or expired" })),
        )
            .into_response(),
    }
}

/// PATCH /sessions/{code} — update session settings (owner only).
async fn update_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> impl IntoResponse {
    let code = code.to_uppercase();
    match state.ns_repo.get_session(&code) {
        Some(session) if session.owner_user_id == auth.user.id => {
            match state.ns_repo.update_session_read_only(&code, req.read_only) {
                Ok(true) => Json(serde_json::json!({
                    "code": code,
                    "read_only": req.read_only,
                }))
                .into_response(),
                Ok(false) => StatusCode::NOT_FOUND.into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response(),
            }
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only the session owner can update it" })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found or expired" })),
        )
            .into_response(),
    }
}

/// DELETE /sessions/{code} — end a session (owner only).
async fn delete_session(
    State(state): State<NsSessionState>,
    RequireAuth(auth): RequireAuth,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let code = code.to_uppercase();
    match state.ns_repo.get_session(&code) {
        Some(session) if session.owner_user_id == auth.user.id => {
            match state.ns_repo.delete_session(&code) {
                Ok(true) => StatusCode::NO_CONTENT.into_response(),
                Ok(false) => StatusCode::NOT_FOUND.into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e })),
                )
                    .into_response(),
            }
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only the session owner can end it" })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Session not found or expired" })),
        )
            .into_response(),
    }
}
