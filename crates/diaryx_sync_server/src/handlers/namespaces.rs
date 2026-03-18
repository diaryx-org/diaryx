//! Namespace CRUD handlers — `POST/GET/DELETE /namespaces`.

use crate::auth::RequireAuth;
use crate::db::{NamespaceInfo, NamespaceRepo};
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Shared state for namespace handlers.
#[derive(Clone)]
pub struct NamespaceState {
    pub ns_repo: Arc<NamespaceRepo>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateNamespaceRequest {
    /// Optional explicit ID (e.g. `"workspace:abc"`). If absent, a UUID is generated.
    pub id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NamespaceResponse {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
}

impl From<NamespaceInfo> for NamespaceResponse {
    fn from(ns: NamespaceInfo) -> Self {
        Self {
            id: ns.id,
            owner_user_id: ns.owner_user_id,
            created_at: ns.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn namespace_routes(state: NamespaceState) -> Router {
    Router::new()
        .route("/", post(create_namespace).get(list_namespaces))
        .route("/{id}", get(get_namespace).delete(delete_namespace))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /namespaces — create a new namespace.
async fn create_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateNamespaceRequest>,
) -> impl IntoResponse {
    let id = req.id.unwrap_or_else(|| Uuid::new_v4().to_string());

    match state.ns_repo.create_namespace(&id, &auth.user.id) {
        Ok(()) => {
            let ns = state.ns_repo.get_namespace(&id).expect("just inserted");
            (StatusCode::CREATED, Json(NamespaceResponse::from(ns))).into_response()
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    100
}

/// GET /namespaces — list namespaces owned by the authenticated user.
async fn list_namespaces(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Query(pagination): Query<PaginationParams>,
) -> impl IntoResponse {
    let limit = pagination.limit.min(500);
    let namespaces: Vec<NamespaceResponse> = state
        .ns_repo
        .list_namespaces(&auth.user.id, limit, pagination.offset)
        .into_iter()
        .map(NamespaceResponse::from)
        .collect();
    Json(namespaces)
}

/// GET /namespaces/{id} — get a single namespace (must be owned by the caller).
async fn get_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.ns_repo.get_namespace(&id) {
        Some(ns) if ns.owner_user_id == auth.user.id => {
            Json(NamespaceResponse::from(ns)).into_response()
        }
        Some(_) => StatusCode::FORBIDDEN.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// DELETE /namespaces/{id} — delete a namespace (must be owned by the caller).
async fn delete_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let ns = match state.ns_repo.get_namespace(&id) {
        Some(ns) => ns,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    if ns.owner_user_id != auth.user.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.ns_repo.delete_namespace(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}
