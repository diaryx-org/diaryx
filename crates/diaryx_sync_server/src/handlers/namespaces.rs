//! Namespace CRUD handlers — `POST/GET/PATCH/DELETE /namespaces`.

use crate::auth::RequireAuth;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
};
use diaryx_server::api::namespaces::{
    CreateNamespaceRequest, NamespaceResponse, UpdateNamespaceRequest,
};
use diaryx_server::ports::{DomainMappingCache, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::namespaces::NamespaceService;
use serde::Deserialize;
use std::sync::Arc;

/// Shared state for namespace handlers.
#[derive(Clone)]
pub struct NamespaceState {
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub domain_mapping_cache: Option<Arc<dyn DomainMappingCache>>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn namespace_routes(state: NamespaceState) -> Router {
    Router::new()
        .route("/", post(create_namespace).get(list_namespaces))
        .route(
            "/{id}",
            get(get_namespace)
                .patch(update_namespace)
                .delete(delete_namespace),
        )
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
    let metadata_str = req.metadata_str();

    match service
        .create(&auth.user.id, req.id.as_deref(), metadata_str.as_deref())
        .await
    {
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

/// PATCH /namespaces/{id} — update namespace metadata (must be owned by the caller).
async fn update_namespace(
    State(state): State<NamespaceState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<String>,
    Json(req): Json<UpdateNamespaceRequest>,
) -> impl IntoResponse {
    let service = NamespaceService::new(state.namespace_store.as_ref());
    let metadata_str = req.metadata_str();

    match service
        .update_metadata(&id, &auth.user.id, metadata_str.as_deref())
        .await
    {
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
    let cache = state.domain_mapping_cache.as_deref();

    match service.delete_with_cache(&id, &auth.user.id, cache).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::NativeNamespaceStore,
        auth::AuthUser,
        db::{NamespaceRepo, init_database},
    };
    use axum::{
        body::to_bytes,
        response::{IntoResponse, Response},
    };
    use chrono::{TimeZone, Utc};
    use diaryx_server::{AuthSessionInfo, UserInfo, UserTier};
    use rusqlite::{Connection, params};
    use serde_json::{Value as JsonValue, json};
    use std::sync::{Arc, Mutex};

    fn setup_repo(users: &[&str]) -> Arc<NamespaceRepo> {
        let conn = Connection::open_in_memory().expect("open sqlite");
        init_database(&conn).expect("init sqlite");
        for user_id in users {
            conn.execute(
                "INSERT INTO users (id, email, created_at, tier) VALUES (?1, ?2, ?3, ?4)",
                params![
                    user_id,
                    format!("{user_id}@example.com"),
                    1_i64,
                    UserTier::Free.as_str()
                ],
            )
            .expect("seed user");
        }
        Arc::new(NamespaceRepo::new(Arc::new(Mutex::new(conn))))
    }

    fn state(repo: Arc<NamespaceRepo>) -> NamespaceState {
        NamespaceState {
            namespace_store: Arc::new(NativeNamespaceStore::new(repo)),
            domain_mapping_cache: None,
        }
    }

    fn auth(user_id: &str) -> RequireAuth {
        RequireAuth(AuthUser {
            session: AuthSessionInfo {
                token: format!("session-{user_id}"),
                user_id: user_id.to_string(),
                device_id: format!("device-{user_id}"),
                expires_at: Utc.timestamp_opt(4_102_444_800, 0).unwrap(),
                created_at: Utc.timestamp_opt(1, 0).unwrap(),
            },
            user: UserInfo {
                id: user_id.to_string(),
                email: format!("{user_id}@example.com"),
                created_at: Utc.timestamp_opt(1, 0).unwrap(),
                last_login_at: None,
                attachment_limit_bytes: None,
                workspace_limit: None,
                tier: UserTier::Plus,
                published_site_limit: None,
            },
        })
    }

    async fn json_body(response: Response) -> JsonValue {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn namespace_routes_cover_crud_lifecycle() {
        let repo = setup_repo(&["user1"]);
        let state = state(repo.clone());

        let created = create_namespace(
            State(state.clone()),
            auth("user1"),
            Json(CreateNamespaceRequest {
                id: Some("workspace:alpha".to_string()),
                metadata: Some(json!({
                    "name": "Alpha",
                    "kind": "workspace"
                })),
            }),
        )
        .await
        .into_response();
        assert_eq!(created.status(), StatusCode::CREATED);
        let created_body = json_body(created).await;
        assert_eq!(created_body["id"], "workspace:alpha");
        assert_eq!(created_body["owner_user_id"], "user1");
        assert_eq!(
            created_body["metadata"],
            json!({
                "kind": "workspace",
                "name": "Alpha"
            })
        );

        let listed = list_namespaces(
            State(state.clone()),
            auth("user1"),
            Query(PaginationParams {
                limit: 10,
                offset: 0,
            }),
        )
        .await
        .into_response();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_body = json_body(listed).await;
        assert_eq!(listed_body.as_array().map(Vec::len), Some(1));

        let fetched = get_namespace(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(fetched.status(), StatusCode::OK);
        let fetched_body = json_body(fetched).await;
        assert_eq!(fetched_body["metadata"]["name"], "Alpha");

        let updated = update_namespace(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
            Json(UpdateNamespaceRequest {
                metadata: Some(json!({
                    "name": "Renamed",
                    "archived": false
                })),
            }),
        )
        .await
        .into_response();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated_body = json_body(updated).await;
        assert_eq!(updated_body["metadata"]["name"], "Renamed");
        assert_eq!(updated_body["metadata"]["archived"], false);

        let deleted = delete_namespace(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let missing = get_namespace(
            State(state),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn namespace_routes_reject_non_owners() {
        let repo = setup_repo(&["owner1"]);
        repo.create_namespace("workspace:alpha", "owner1", None)
            .expect("seed namespace");
        let state = state(repo);

        let response = get_namespace(
            State(state),
            auth("intruder"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert_eq!(body["error"], "You do not own this namespace");
    }
}
