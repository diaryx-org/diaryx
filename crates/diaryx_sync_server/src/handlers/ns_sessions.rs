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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::{NativeNamespaceStore, NativeSessionStore},
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
    use serde_json::Value as JsonValue;
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

    fn state(repo: Arc<NamespaceRepo>) -> NsSessionState {
        NsSessionState {
            namespace_store: Arc::new(NativeNamespaceStore::new(repo.clone())),
            session_store: Arc::new(NativeSessionStore::new(repo)),
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
    async fn session_routes_cover_lifecycle() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        let state = state(repo);

        let created = create_session(
            State(state.clone()),
            auth("user1"),
            Json(CreateSessionRequest {
                namespace_id: "workspace:alpha".to_string(),
                read_only: false,
            }),
        )
        .await
        .into_response();
        assert_eq!(created.status(), StatusCode::OK);
        let created_body = json_body(created).await;
        let code = created_body["code"]
            .as_str()
            .expect("session code")
            .to_string();
        assert_eq!(created_body["namespace_id"], "workspace:alpha");
        assert_eq!(created_body["read_only"], false);

        let fetched = get_session(State(state.clone()), Path(code.clone()))
            .await
            .into_response();
        assert_eq!(fetched.status(), StatusCode::OK);
        let fetched_body = json_body(fetched).await;
        assert_eq!(fetched_body["code"], code);

        let updated = update_session(
            State(state.clone()),
            auth("user1"),
            Path(code.to_lowercase()),
            Json(UpdateSessionRequest { read_only: true }),
        )
        .await
        .into_response();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated_body = json_body(updated).await;
        assert_eq!(updated_body["code"], code);
        assert_eq!(updated_body["read_only"], true);

        let deleted = delete_session(State(state.clone()), auth("user1"), Path(code.clone()))
            .await
            .into_response();
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let missing = get_session(State(state), Path(code)).await.into_response();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn session_routes_reject_non_owners() {
        let repo = setup_repo(&["owner1"]);
        repo.create_namespace("workspace:alpha", "owner1", None)
            .expect("seed namespace");
        let state = state(repo);

        let created = create_session(
            State(state.clone()),
            auth("owner1"),
            Json(CreateSessionRequest {
                namespace_id: "workspace:alpha".to_string(),
                read_only: false,
            }),
        )
        .await
        .into_response();
        let created_body = json_body(created).await;
        let code = created_body["code"]
            .as_str()
            .expect("session code")
            .to_string();

        let response = update_session(
            State(state),
            auth("intruder"),
            Path(code),
            Json(UpdateSessionRequest { read_only: true }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert_eq!(body["error"], "Only the session owner can update it");
    }
}
