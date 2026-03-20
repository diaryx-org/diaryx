//! Namespace CRUD handlers — `POST/GET/DELETE /namespaces`.

use crate::auth::RequireAuth;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use diaryx_server::domain::NamespaceInfo;
use diaryx_server::ports::{NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::namespaces::NamespaceService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state for namespace handlers.
#[derive(Clone)]
pub struct NamespaceState {
    pub namespace_store: Arc<dyn NamespaceStore>,
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
// Helpers
// ---------------------------------------------------------------------------

fn status_for_core_error(err: &ServerCoreError) -> StatusCode {
    match err {
        ServerCoreError::InvalidInput(_) | ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
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

/// POST /namespaces — create a new namespace.
async fn create_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateNamespaceRequest>,
) -> impl IntoResponse {
    let service = NamespaceService::new(state.namespace_store.as_ref());

    match service.create(&auth.user.id, req.id.as_deref()).await {
        Ok(ns) => (StatusCode::CREATED, Json(NamespaceResponse::from(ns))).into_response(),
        Err(e) => core_error_response(e),
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
    let service = NamespaceService::new(state.namespace_store.as_ref());

    match service
        .list(&auth.user.id, pagination.limit, pagination.offset)
        .await
    {
        Ok(namespaces) => {
            let response: Vec<NamespaceResponse> = namespaces
                .into_iter()
                .map(NamespaceResponse::from)
                .collect();
            Json(response).into_response()
        }
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{id} — get a single namespace (must be owned by the caller).
async fn get_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let service = NamespaceService::new(state.namespace_store.as_ref());

    match service.get(&id, &auth.user.id).await {
        Ok(ns) => Json(NamespaceResponse::from(ns)).into_response(),
        Err(e) => core_error_response(e),
    }
}

/// DELETE /namespaces/{id} — delete a namespace (must be owned by the caller).
async fn delete_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let service = NamespaceService::new(state.namespace_store.as_ref());

    match service.delete(&id, &auth.user.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}
