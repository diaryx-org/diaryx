use crate::auth::RequireAuth;
use crate::db::AuthRepo;
use crate::sync_v2::{SnapshotImportMode, SyncV2State};
use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
    routing::{get, post},
};
use serde::Serialize;
use std::sync::Arc;
use tracing::{error, info};

const SNAPSHOT_UPLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;

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

/// Git commit log entry
#[derive(Debug, Serialize)]
pub struct CommitLogEntry {
    pub id: String,
    pub short_id: String,
    pub message: String,
    pub timestamp: String,
    pub file_count: usize,
}

/// Git commit response
#[derive(Debug, Serialize)]
pub struct CommitResponse {
    pub commit_id: String,
    pub file_count: usize,
    pub compacted: bool,
}

/// Restore response
#[derive(Debug, Serialize)]
pub struct RestoreResponse {
    pub restored_from: String,
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
            get(get_workspace_snapshot)
                .post(upload_workspace_snapshot)
                .layer(
                    // Snapshot ZIP payloads can exceed axum's 2MiB default body limit.
                    // Keep an explicit cap to prevent unbounded uploads.
                    DefaultBodyLimit::max(SNAPSHOT_UPLOAD_MAX_BYTES),
                ),
        )
        .route(
            "/workspaces/{workspace_id}/history",
            get(get_workspace_history),
        )
        .route(
            "/workspaces/{workspace_id}/commit",
            post(trigger_workspace_commit),
        )
        .route(
            "/workspaces/{workspace_id}/restore",
            post(restore_workspace),
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

/// GET /api/workspaces/:workspace_id/history - Get git commit history
async fn get_workspace_history(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let repo_path = state.sync_v2.storage_cache.git_repo_path(&workspace_id);
    let repo = match git2::Repository::open(&repo_path) {
        Ok(r) => r,
        Err(_) => return Json(Vec::<CommitLogEntry>::new()).into_response(),
    };

    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return Json(Vec::<CommitLogEntry>::new()).into_response(),
    };

    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to walk commits for {}: {}", workspace_id, e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if revwalk.push(head.target().unwrap()).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let count = query.count.unwrap_or(20).min(100);
    let mut entries = Vec::new();

    for oid in revwalk {
        if entries.len() >= count {
            break;
        }
        let oid = match oid {
            Ok(o) => o,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_count = commit.tree().map(|t| t.len()).unwrap_or(0);

        let time = commit.time();
        let dt = chrono::DateTime::from_timestamp(time.seconds(), 0).unwrap_or_default();

        entries.push(CommitLogEntry {
            id: oid.to_string(),
            short_id: oid.to_string()[..8].to_string(),
            message: commit
                .message()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            timestamp: dt.to_rfc3339(),
            file_count,
        });
    }

    Json(entries).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct HistoryQuery {
    count: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct CommitMessage {
    message: Option<String>,
}

/// POST /api/workspaces/:workspace_id/commit - Trigger an immediate git commit
async fn trigger_workspace_commit(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Json(body): Json<CommitMessage>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    match crate::git_ops::commit_workspace_by_id(
        &state.sync_v2.storage_cache,
        &workspace_id,
        body.message,
    ) {
        Ok(result) => {
            // Clear dirty flag since we just committed
            state
                .sync_v2
                .dirty_workspaces
                .write()
                .await
                .remove(&workspace_id);

            info!(
                "Manual commit for workspace {}: {} files [{}]",
                workspace_id, result.file_count, result.commit_id
            );

            Json(CommitResponse {
                commit_id: result.commit_id.to_string(),
                file_count: result.file_count,
                compacted: result.compacted,
            })
            .into_response()
        }
        Err(e) => {
            error!("Commit failed for {}: {}", workspace_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct RestoreRequest {
    commit_id: String,
}

/// POST /api/workspaces/:workspace_id/restore - Restore CRDT from a git commit
async fn restore_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Json(body): Json<RestoreRequest>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Check peer count — only allow restore when no other clients are connected
    let doc_id = format!("workspace:{}", workspace_id);
    let peer_count = state.sync_v2.handle.get_peer_count(&doc_id).await;
    if peer_count > 1 {
        return (
            StatusCode::CONFLICT,
            format!(
                "Cannot restore while {} peers are connected. Disconnect other clients first.",
                peer_count
            ),
        )
            .into_response();
    }

    let oid = match git2::Oid::from_str(&body.commit_id) {
        Ok(o) => o,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Invalid commit ID").into_response();
        }
    };

    match crate::git_ops::restore_workspace_by_id(&state.sync_v2.storage_cache, &workspace_id, oid)
    {
        Ok(file_count) => {
            info!(
                "Restored workspace {} from commit {} ({} files)",
                workspace_id, body.commit_id, file_count
            );

            Json(RestoreResponse {
                restored_from: body.commit_id,
                file_count,
            })
            .into_response()
        }
        Err(e) => {
            error!(
                "Restore failed for {} from {}: {}",
                workspace_id, body.commit_id, e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
