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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::NativeNamespaceStore,
        auth::AuthUser,
        blob_store::InMemoryBlobStore,
        db::{NamespaceRepo, init_database},
        tokens::validate_signed_token,
    };
    use axum::{
        body::to_bytes,
        response::{IntoResponse, Response},
    };
    use chrono::{TimeZone, Utc};
    use diaryx_server::{AuthSessionInfo, BlobStore, UserInfo, UserTier};
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

    fn state(repo: Arc<NamespaceRepo>, blob_store: Arc<InMemoryBlobStore>) -> AudienceState {
        AudienceState {
            namespace_store: Arc::new(NativeNamespaceStore::new(repo)),
            token_signing_key: b"audience-signing-key".to_vec(),
            blob_store,
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
    async fn audience_routes_cover_lifecycle_and_token_generation() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        let blob_store = Arc::new(InMemoryBlobStore::new(""));
        let state = state(repo, blob_store.clone());

        let set_response = set_audience(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "members".to_string())),
            Json(SetAudienceRequest {
                access: "token".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(set_response.status(), StatusCode::OK);
        let set_body = json_body(set_response).await;
        assert_eq!(set_body["name"], "members");
        assert_eq!(set_body["access"], "token");

        let listed = list_audiences(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed_body = json_body(listed).await;
        assert_eq!(listed_body.as_array().map(Vec::len), Some(1));

        let token_response = get_audience_token(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "members".to_string())),
        )
        .await
        .into_response();
        assert_eq!(token_response.status(), StatusCode::OK);
        let token_body = json_body(token_response).await;
        let token = token_body["token"].as_str().expect("signed token");
        let claims = validate_signed_token(&state.token_signing_key, token).expect("claims");
        assert_eq!(claims.slug, "workspace:alpha");
        assert_eq!(claims.audience, "members");

        let metadata_blob = blob_store
            .get("ns/workspace:alpha/_audiences.json")
            .await
            .expect("blob get")
            .expect("metadata blob");
        let metadata_json: JsonValue =
            serde_json::from_slice(&metadata_blob).expect("metadata json");
        assert_eq!(metadata_json, json!({ "members": "token" }));

        let deleted = delete_audience(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "members".to_string())),
        )
        .await
        .into_response();
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let empty = list_audiences(
            State(state),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        let empty_body = json_body(empty).await;
        assert_eq!(empty_body, json!([]));
    }

    #[tokio::test]
    async fn audience_token_route_rejects_public_audiences() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        let state = state(repo, Arc::new(InMemoryBlobStore::new("")));

        let _ = set_audience(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "public".to_string())),
            Json(SetAudienceRequest {
                access: "public".to_string(),
            }),
        )
        .await;

        let response = get_audience_token(
            State(state),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "public".to_string())),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = json_body(response).await;
        assert_eq!(body["error"], "audience is public; no token needed");
    }
}
