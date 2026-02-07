use crate::auth::RequireAuth;
use crate::db::AuthRepo;
use crate::sync_v2::{SnapshotImportMode, SyncV2State};
use axum::body::Bytes;
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
    routing::get,
};
use serde::Serialize;
use std::sync::Arc;
use tracing::error;

/// Shared state for API handlers
#[derive(Clone)]
pub struct ApiState {
    pub repo: Arc<AuthRepo>,
    pub sync_v2: Arc<SyncV2State>,
}

/// Server status response
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub active_connections: usize,
    pub active_rooms: usize,
}

/// Workspace info response
#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
}

/// User has data response
#[derive(Debug, Serialize)]
pub struct UserHasDataResponse {
    pub has_data: bool,
    pub file_count: usize,
}

/// Create API routes
pub fn api_routes(state: ApiState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/workspaces", get(list_workspaces))
        .route("/workspaces/{workspace_id}", get(get_workspace))
        .route(
            "/workspaces/{workspace_id}/snapshot",
            get(get_workspace_snapshot).post(upload_workspace_snapshot),
        )
        .route("/user/has-data", get(check_user_has_data))
        .with_state(state)
}

/// GET /api/status - Get server status (public endpoint)
async fn get_status(State(_state): State<ApiState>) -> impl IntoResponse {
    // Siphonophore doesn't expose global stats; return version info only
    Json(StatusResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_connections: 0,
        active_rooms: 0,
    })
}

/// GET /api/workspaces - List user's workspaces
async fn list_workspaces(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let workspaces = state
        .repo
        .get_user_workspaces(&auth.user.id)
        .unwrap_or_default()
        .into_iter()
        .map(|w| WorkspaceResponse {
            id: w.id,
            name: w.name,
        })
        .collect::<Vec<_>>();

    Json(workspaces)
}

/// GET /api/workspaces/:workspace_id - Get workspace info
async fn get_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Verify ownership
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    Json(WorkspaceResponse {
        id: workspace.id,
        name: workspace.name,
    })
    .into_response()
}

/// GET /api/workspaces/:workspace_id/snapshot - Download workspace snapshot zip
async fn get_workspace_snapshot(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Verify ownership
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let snapshot = match state.sync_v2.store.export_snapshot_zip(&workspace_id) {
        Ok(bytes) => bytes,
        Err(err) => {
            error!("Snapshot export failed for {}: {:?}", workspace_id, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/zip".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!(
            "attachment; filename=\"diaryx-snapshot-{}.zip\"",
            workspace_id
        )
        .parse()
        .unwrap(),
    );

    (headers, snapshot).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct SnapshotUploadQuery {
    mode: Option<String>,
}

/// POST /api/workspaces/:workspace_id/snapshot - Upload workspace snapshot zip
async fn upload_workspace_snapshot(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Query(query): Query<SnapshotUploadQuery>,
    bytes: Bytes,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let mode = match query.mode.as_deref() {
        Some("merge") => SnapshotImportMode::Merge,
        _ => SnapshotImportMode::Replace,
    };

    let result = match state
        .sync_v2
        .store
        .import_snapshot_zip(&workspace_id, &bytes, mode)
    {
        Ok(result) => result,
        Err(err) => {
            error!("Snapshot import failed for {}: {:?}", workspace_id, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(result).into_response()
}

/// GET /api/user/has-data - Check if user has synced data on the server
async fn check_user_has_data(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    // Get user's workspaces
    let workspaces = state
        .repo
        .get_user_workspaces(&auth.user.id)
        .unwrap_or_default();

    // Look for the default workspace
    let default_ws = workspaces.into_iter().find(|w| w.name == "default");

    if let Some(ws) = default_ws {
        let count = state.sync_v2.store.get_file_count(&ws.id);
        return Json(UserHasDataResponse {
            has_data: count > 0,
            file_count: count,
        });
    }

    // No workspace found - user has no data
    Json(UserHasDataResponse {
        has_data: false,
        file_count: 0,
    })
}
