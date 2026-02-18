use crate::auth::RequireAuth;
use crate::blob_store::{BlobStore, MultipartCompletedPart};
use crate::db::{AttachmentUploadSession, AuthRepo};
use crate::rate_limit::RateLimiter;
use crate::sync_v2::{SnapshotImportMode, SyncV2State};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
    routing::{get, post, put},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};

const ATTACHMENT_PART_SIZE_DEFAULT: u64 = 8 * 1024 * 1024;
const ATTACHMENT_UPLOAD_SESSION_TTL_SECS: i64 = 24 * 60 * 60;

/// Shared state for API handlers
#[derive(Clone)]
pub struct ApiState {
    pub repo: Arc<AuthRepo>,
    pub sync_v2: Arc<SyncV2State>,
    pub blob_store: Arc<dyn BlobStore>,
    pub snapshot_upload_max_bytes: usize,
    pub attachment_incremental_sync_enabled: bool,
    pub admin_secret: Option<String>,
    pub rate_limiter: RateLimiter,
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

/// User storage usage response.
#[derive(Debug, Serialize)]
pub struct UserStorageResponse {
    pub used_bytes: u64,
    pub blob_count: usize,
    pub limit_bytes: Option<u64>,
    pub warning_threshold: f64,
    pub over_limit: bool,
    pub scope: String,
}

#[derive(Debug, Serialize)]
struct StorageLimitErrorResponse {
    error: String,
    message: String,
    used_bytes: u64,
    limit_bytes: u64,
    requested_bytes: u64,
}

fn storage_limit_exceeded_response(
    used_bytes: u64,
    limit_bytes: u64,
    requested_bytes: u64,
) -> axum::response::Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(StorageLimitErrorResponse {
            error: "storage_limit_exceeded".to_string(),
            message: "Attachment storage limit exceeded".to_string(),
            used_bytes,
            limit_bytes,
            requested_bytes,
        }),
    )
        .into_response()
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
        .route("/workspaces", get(list_workspaces).post(create_workspace))
        .route(
            "/workspaces/{workspace_id}",
            get(get_workspace)
                .patch(rename_workspace)
                .delete(delete_workspace),
        )
        .route(
            "/workspaces/{workspace_id}/snapshot",
            get(get_workspace_snapshot)
                .post(upload_workspace_snapshot)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/workspaces/{workspace_id}/attachments/uploads",
            post(init_attachment_upload),
        )
        .route(
            "/workspaces/{workspace_id}/attachments/uploads/{upload_id}/parts/{part_no}",
            put(upload_attachment_part).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/workspaces/{workspace_id}/attachments/uploads/{upload_id}/complete",
            post(complete_attachment_upload),
        )
        .route(
            "/workspaces/{workspace_id}/attachments/{blob_hash}",
            get(download_attachment_blob),
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
        .route("/user/storage", get(get_user_storage))
        .route("/admin/users/{user_id}/tier", put(set_user_tier))
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

#[derive(Debug, Deserialize)]
struct CreateWorkspaceRequest {
    name: String,
}

/// POST /api/workspaces - Create a new workspace
async fn create_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    Json(body): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 100 {
        return (StatusCode::BAD_REQUEST, "Invalid workspace name").into_response();
    }

    match state.repo.create_workspace(&auth.user.id, name) {
        Ok(Ok(workspace_id)) => Json(WorkspaceResponse {
            id: workspace_id,
            name: name.to_string(),
        })
        .into_response(),
        Ok(Err(limit_msg)) => (StatusCode::FORBIDDEN, limit_msg).into_response(),
        Err(err) => {
            // Check for unique constraint violation (duplicate name)
            let err_str = err.to_string();
            if err_str.contains("UNIQUE constraint") {
                return (
                    StatusCode::CONFLICT,
                    "A workspace with that name already exists",
                )
                    .into_response();
            }
            error!("Failed to create workspace: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct RenameWorkspaceRequest {
    name: String,
}

/// PATCH /api/workspaces/:workspace_id - Rename a workspace
async fn rename_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Json(body): Json<RenameWorkspaceRequest>,
) -> impl IntoResponse {
    let new_name = body.name.trim();
    if new_name.is_empty() || new_name.len() > 100 {
        return (StatusCode::BAD_REQUEST, "Invalid workspace name").into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    match state.repo.rename_workspace(&workspace_id, new_name) {
        Ok(true) => Json(WorkspaceResponse {
            id: workspace_id,
            name: new_name.to_string(),
        })
        .into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            let err_str = err.to_string();
            if err_str.contains("UNIQUE constraint") {
                return (
                    StatusCode::CONFLICT,
                    "A workspace with that name already exists",
                )
                    .into_response();
            }
            error!("Failed to rename workspace: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// DELETE /api/workspaces/:workspace_id - Delete a workspace and all its data
async fn delete_workspace(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    // Decrement blob ref counts before deleting the workspace row
    if let Err(err) = state
        .repo
        .replace_workspace_attachment_refs(&workspace_id, &[])
    {
        error!(
            "Failed to clear attachment refs for workspace {}: {}",
            workspace_id, err
        );
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    // Delete workspace row (CASCADE handles share_sessions, attachment_refs, etc.)
    match state.repo.delete_workspace(&workspace_id) {
        Ok(true) => {}
        Ok(false) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Failed to delete workspace {}: {}", workspace_id, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    // Clear pending auto-commit state for the deleted workspace.
    state
        .sync_v2
        .dirty_workspaces
        .write()
        .await
        .remove(&workspace_id);

    // Evict cached storage handle before removing the database file.
    state.sync_v2.storage_cache.evict_storage(&workspace_id);

    let db_path = state.sync_v2.storage_cache.workspace_db_path(&workspace_id);
    if db_path.exists() {
        if let Err(err) = std::fs::remove_file(&db_path) {
            warn!(
                "Failed to remove CRDT DB for deleted workspace {}: {}",
                workspace_id, err
            );
        }
    }

    // Clean up git repo directory
    let git_path = state.sync_v2.storage_cache.git_repo_path(&workspace_id);
    if git_path.exists() {
        if let Err(err) = std::fs::remove_dir_all(&git_path) {
            warn!(
                "Failed to remove git repo for deleted workspace {}: {}",
                workspace_id, err
            );
        }
    }

    info!(
        "Deleted workspace {} (name: {}) for user {}",
        workspace_id, workspace.name, auth.user.id
    );

    StatusCode::NO_CONTENT.into_response()
}

/// GET /api/workspaces/:workspace_id/snapshot - Download workspace snapshot zip
async fn get_workspace_snapshot(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Query(query): Query<SnapshotQuery>,
) -> impl IntoResponse {
    // Rate limit: 5 per hour
    if let Err(retry_after) = state.rate_limiter.check(
        &auth.user.id,
        "snapshot_export",
        5,
        Duration::from_secs(3600),
    ) {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            "Rate limit exceeded",
        )
            .into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    // Verify ownership
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let include_attachments = query.include_attachments.unwrap_or(true);
    let temp_file = match state
        .sync_v2
        .store
        .export_snapshot_zip_to_file(
            &workspace_id,
            &auth.user.id,
            include_attachments,
            state.blob_store.as_ref(),
        )
        .await
    {
        Ok(f) => f,
        Err(err) => {
            error!("Snapshot export failed for {}: {:?}", workspace_id, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Stream the temp file — on Unix, NamedTempFile drop unlinks the file but
    // the open fd keeps it readable until the stream completes.
    let file = match tokio::fs::File::open(temp_file.path()).await {
        Ok(f) => f,
        Err(err) => {
            error!("Failed to open temp snapshot file: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

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

    // Keep temp_file alive until response is built — the Body holds an open fd,
    // so the file data remains accessible even after unlink.
    let _keep = temp_file;

    (headers, body).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct SnapshotQuery {
    include_attachments: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct SnapshotUploadQuery {
    mode: Option<String>,
    include_attachments: Option<bool>,
}

/// POST /api/workspaces/:workspace_id/snapshot - Upload workspace snapshot zip
async fn upload_workspace_snapshot(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Query(query): Query<SnapshotUploadQuery>,
    request: Request,
) -> impl IntoResponse {
    // Rate limit: 5 per hour
    if let Err(retry_after) = state.rate_limiter.check(
        &auth.user.id,
        "snapshot_import",
        5,
        Duration::from_secs(3600),
    ) {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            "Rate limit exceeded",
        )
            .into_response();
    }

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
    let include_attachments = query.include_attachments.unwrap_or(true);

    let temp_path = std::env::temp_dir().join(format!(
        "diaryx-snapshot-{}-{}.zip",
        workspace_id,
        uuid::Uuid::new_v4()
    ));

    let mut file = match tokio::fs::File::create(&temp_path).await {
        Ok(f) => f,
        Err(err) => {
            error!("Failed to create temp snapshot file: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut size = 0usize;
    let mut stream = request.into_body().into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(err) => {
                error!("Snapshot upload stream read failed: {}", err);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return StatusCode::BAD_REQUEST.into_response();
            }
        };

        size += chunk.len();
        if size > state.snapshot_upload_max_bytes {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }

        if let Err(err) = file.write_all(&chunk).await {
            error!("Failed writing snapshot upload chunk: {}", err);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Err(err) = file.flush().await {
        error!("Failed flushing snapshot temp file: {}", err);
        let _ = tokio::fs::remove_file(&temp_path).await;
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(file);

    let result = match state
        .sync_v2
        .store
        .import_snapshot_zip_from_path(
            &workspace_id,
            &auth.user.id,
            &temp_path,
            mode,
            include_attachments,
            state.repo.as_ref(),
            state.blob_store.as_ref(),
        )
        .await
    {
        Ok(result) => result,
        Err(crate::sync_v2::SnapshotError::QuotaExceeded {
            used_bytes,
            limit_bytes,
            requested_bytes,
        }) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return storage_limit_exceeded_response(used_bytes, limit_bytes, requested_bytes);
        }
        Err(err) => {
            error!("Snapshot import failed for {}: {:?}", workspace_id, err);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let _ = tokio::fs::remove_file(&temp_path).await;

    Json(result).into_response()
}

#[derive(Debug, Deserialize)]
struct InitAttachmentUploadRequest {
    attachment_path: String,
    hash: String,
    size_bytes: u64,
    mime_type: String,
    part_size: Option<u64>,
    total_parts: Option<u32>,
}

#[derive(Debug, Serialize)]
struct InitAttachmentUploadResponse {
    upload_id: Option<String>,
    status: String,
    part_size: u64,
    uploaded_parts: Vec<u32>,
}

#[derive(Debug, Serialize)]
struct AttachmentPartUploadResponse {
    ok: bool,
    part_no: u32,
}

#[derive(Debug, Deserialize)]
struct CompleteAttachmentUploadRequest {
    attachment_path: String,
    hash: String,
    size_bytes: u64,
    mime_type: String,
}

#[derive(Debug, Serialize)]
struct CompleteAttachmentUploadResponse {
    ok: bool,
    blob_hash: String,
    r2_key: String,
    missing_parts: Option<Vec<u32>>,
}

fn parse_range_header(headers: &HeaderMap, total_size: u64) -> Option<(u64, u64)> {
    let raw = headers.get(header::RANGE)?.to_str().ok()?;
    if !raw.starts_with("bytes=") {
        return None;
    }
    let range = &raw[6..];
    let (start_s, end_s) = range.split_once('-')?;
    let start = start_s.parse::<u64>().ok()?;
    let end = if end_s.is_empty() {
        total_size.saturating_sub(1)
    } else {
        end_s.parse::<u64>().ok()?
    };
    if total_size == 0 || start > end || end >= total_size {
        return None;
    }
    Some((start, end))
}

fn hash_looks_like_sha256(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// POST /api/workspaces/:workspace_id/attachments/uploads - init resumable upload.
async fn init_attachment_upload(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Json(body): Json<InitAttachmentUploadRequest>,
) -> impl IntoResponse {
    if !state.attachment_incremental_sync_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    if !hash_looks_like_sha256(&body.hash)
        || body.size_bytes == 0
        || body.mime_type.trim().is_empty()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let part_size = body.part_size.unwrap_or(ATTACHMENT_PART_SIZE_DEFAULT);
    if part_size == 0 || part_size > 32 * 1024 * 1024 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let derived_parts = body.size_bytes.div_ceil(part_size) as u32;
    let total_parts = body.total_parts.unwrap_or(derived_parts);
    if total_parts == 0 || total_parts != derived_parts {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // Small uploads that fit in one part use direct object put to avoid
    // unnecessary multipart lifecycle overhead.
    let use_direct_put = total_parts == 1;

    if let Ok(Some((_key, _size, _mime))) = state.repo.get_user_blob(&auth.user.id, &body.hash) {
        return Json(InitAttachmentUploadResponse {
            upload_id: None,
            status: "already_exists".to_string(),
            part_size,
            uploaded_parts: vec![],
        })
        .into_response();
    }
    let limit_bytes = match state
        .repo
        .get_effective_user_attachment_limit(&auth.user.id)
    {
        Ok(v) => v,
        Err(err) => {
            error!("Failed to get user attachment limit: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let (used_bytes, projected, is_new_blob) = match state.repo.compute_projected_usage_after_blob(
        &auth.user.id,
        &body.hash,
        body.size_bytes,
    ) {
        Ok(v) => v,
        Err(err) => {
            error!("Failed to compute projected storage usage: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if is_new_blob && projected > limit_bytes {
        return storage_limit_exceeded_response(used_bytes, limit_bytes, body.size_bytes);
    }

    let upload_id = uuid::Uuid::new_v4().to_string();
    let r2_key = state.blob_store.blob_key(&auth.user.id, &body.hash);
    let multipart_id = if use_direct_put {
        String::new()
    } else {
        match state
            .blob_store
            .init_multipart(&r2_key, &body.mime_type)
            .await
        {
            Ok(id) => id,
            Err(err) => {
                error!("Attachment upload init failed: {}", err);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    let now = chrono::Utc::now().timestamp();
    let session = AttachmentUploadSession {
        upload_id: upload_id.clone(),
        workspace_id: workspace_id.clone(),
        user_id: auth.user.id.clone(),
        blob_hash: body.hash,
        attachment_path: body.attachment_path,
        mime_type: body.mime_type,
        size_bytes: body.size_bytes,
        part_size,
        total_parts,
        r2_key,
        r2_multipart_upload_id: multipart_id,
        status: "uploading".to_string(),
        created_at: now,
        updated_at: now,
        expires_at: now + ATTACHMENT_UPLOAD_SESSION_TTL_SECS,
    };
    if let Err(err) = state.repo.create_attachment_upload_session(&session) {
        error!("Attachment upload session DB insert failed: {}", err);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(InitAttachmentUploadResponse {
        upload_id: Some(upload_id),
        status: "uploading".to_string(),
        part_size,
        uploaded_parts: vec![],
    })
    .into_response()
}

/// PUT /api/workspaces/:workspace_id/attachments/uploads/:upload_id/parts/:part_no
async fn upload_attachment_part(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path((workspace_id, upload_id, part_no)): axum::extract::Path<(
        String,
        String,
        u32,
    )>,
    body: Bytes,
) -> impl IntoResponse {
    if !state.attachment_incremental_sync_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let session = match state.repo.get_attachment_upload_session(&upload_id) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Attachment upload session lookup failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if session.workspace_id != workspace_id || session.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }
    if session.status != "uploading" {
        return StatusCode::CONFLICT.into_response();
    }
    if chrono::Utc::now().timestamp() > session.expires_at {
        return StatusCode::GONE.into_response();
    }
    if part_no == 0 || part_no > session.total_parts {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if body.len() as u64 > session.part_size {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let etag = if session.r2_multipart_upload_id.is_empty() {
        if session.total_parts != 1 || part_no != 1 || body.len() as u64 != session.size_bytes {
            return StatusCode::BAD_REQUEST.into_response();
        }
        if let Err(err) = state
            .blob_store
            .put(&session.r2_key, &body, &session.mime_type)
            .await
        {
            error!("Attachment single-part direct upload failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        "direct-put".to_string()
    } else {
        match state
            .blob_store
            .upload_part(
                &session.r2_key,
                &session.r2_multipart_upload_id,
                part_no,
                &body,
            )
            .await
        {
            Ok(etag) => etag,
            Err(err) => {
                error!("Attachment multipart part upload failed: {}", err);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };
    if let Err(err) =
        state
            .repo
            .upsert_attachment_upload_part(&upload_id, part_no, &etag, body.len() as u64)
    {
        error!("Attachment upload part DB write failed: {}", err);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    Json(AttachmentPartUploadResponse { ok: true, part_no }).into_response()
}

/// POST /api/workspaces/:workspace_id/attachments/uploads/:upload_id/complete
async fn complete_attachment_upload(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path((workspace_id, upload_id)): axum::extract::Path<(String, String)>,
    Json(body): Json<CompleteAttachmentUploadRequest>,
) -> impl IntoResponse {
    if !state.attachment_incremental_sync_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }

    let session = match state.repo.get_attachment_upload_session(&upload_id) {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Attachment upload session lookup failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if session.workspace_id != workspace_id || session.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }
    if session.status != "uploading" {
        return StatusCode::CONFLICT.into_response();
    }
    if body.hash != session.blob_hash
        || body.size_bytes != session.size_bytes
        || body.mime_type != session.mime_type
        || body.attachment_path != session.attachment_path
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let uploaded_parts = match state.repo.list_attachment_upload_parts(&upload_id) {
        Ok(parts) => parts,
        Err(err) => {
            error!("Attachment upload part list failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let uploaded_set = uploaded_parts
        .iter()
        .map(|p| p.part_no)
        .collect::<std::collections::HashSet<_>>();
    let missing_parts = (1..=session.total_parts)
        .filter(|part| !uploaded_set.contains(part))
        .collect::<Vec<_>>();
    if !missing_parts.is_empty() {
        return (
            StatusCode::CONFLICT,
            Json(CompleteAttachmentUploadResponse {
                ok: false,
                blob_hash: session.blob_hash,
                r2_key: session.r2_key,
                missing_parts: Some(missing_parts),
            }),
        )
            .into_response();
    }

    let limit_bytes = match state
        .repo
        .get_effective_user_attachment_limit(&auth.user.id)
    {
        Ok(v) => v,
        Err(err) => {
            error!("Failed to get user attachment limit: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let (used_bytes, projected, is_new_blob) = match state.repo.compute_projected_usage_after_blob(
        &auth.user.id,
        &session.blob_hash,
        session.size_bytes,
    ) {
        Ok(v) => v,
        Err(err) => {
            error!("Failed to compute projected storage usage: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if is_new_blob && projected > limit_bytes {
        if !session.r2_multipart_upload_id.is_empty() {
            if let Err(err) = state
                .blob_store
                .abort_multipart(&session.r2_key, &session.r2_multipart_upload_id)
                .await
            {
                error!("Failed to abort over-limit multipart upload: {}", err);
            }
        }
        if let Err(err) = state
            .repo
            .set_attachment_upload_status(&upload_id, "aborted")
        {
            error!(
                "Failed to set aborted status for over-limit upload {}: {}",
                upload_id, err
            );
        }
        return storage_limit_exceeded_response(used_bytes, limit_bytes, session.size_bytes);
    }

    if !session.r2_multipart_upload_id.is_empty() {
        let completed_parts = uploaded_parts
            .iter()
            .map(|part| MultipartCompletedPart {
                part_no: part.part_no,
                etag: part.etag.clone(),
            })
            .collect::<Vec<_>>();
        if let Err(err) = state
            .blob_store
            .complete_multipart(
                &session.r2_key,
                &session.r2_multipart_upload_id,
                &completed_parts,
            )
            .await
        {
            error!(
                "Attachment multipart completion failed for upload_id={} key={} parts={}: {}",
                upload_id,
                session.r2_key,
                completed_parts.len(),
                err
            );
            if let Err(abort_err) = state
                .blob_store
                .abort_multipart(&session.r2_key, &session.r2_multipart_upload_id)
                .await
            {
                error!(
                    "Attachment multipart abort after completion failure failed for upload_id={}: {}",
                    upload_id, abort_err
                );
            }
            if let Err(status_err) = state
                .repo
                .set_attachment_upload_status(&upload_id, "failed")
            {
                error!(
                    "Attachment upload status update to failed failed for {}: {}",
                    upload_id, status_err
                );
            }
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    if let Err(err) = state.repo.upsert_blob(
        &auth.user.id,
        &session.blob_hash,
        &session.r2_key,
        session.size_bytes,
        &session.mime_type,
    ) {
        error!("Attachment blob upsert failed: {}", err);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(err) = state
        .repo
        .set_attachment_upload_status(&upload_id, "completed")
    {
        error!("Attachment upload status update failed: {}", err);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    match state
        .sync_v2
        .store
        .reconcile_workspace_attachment_refs(&workspace_id, &state.repo)
    {
        Ok(ref_count) => {
            info!(
                "Attachment reconciliation complete for {} after upload {} ({} refs)",
                workspace_id, upload_id, ref_count
            );
        }
        Err(err) => {
            warn!(
                "Attachment reconciliation after upload completion failed for {} (upload {}): {}",
                workspace_id, upload_id, err
            );
        }
    }

    Json(CompleteAttachmentUploadResponse {
        ok: true,
        blob_hash: session.blob_hash,
        r2_key: session.r2_key,
        missing_parts: None,
    })
    .into_response()
}

/// GET /api/workspaces/:workspace_id/attachments/:blob_hash
async fn download_attachment_blob(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path((workspace_id, blob_hash)): axum::extract::Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.attachment_incremental_sync_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    let workspace = match state.repo.get_workspace(&workspace_id) {
        Ok(Some(w)) => w,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if workspace.user_id != auth.user.id {
        return StatusCode::NOT_FOUND.into_response();
    }
    let is_referenced = match state
        .repo
        .workspace_references_blob(&workspace_id, &blob_hash)
    {
        Ok(value) => value,
        Err(err) => {
            error!("Attachment ref lookup failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if !is_referenced {
        return StatusCode::NOT_FOUND.into_response();
    }

    let (r2_key, size_bytes, mime_type) = match state.repo.get_user_blob(&auth.user.id, &blob_hash)
    {
        Ok(Some(row)) => row,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Attachment blob metadata lookup failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
    response_headers.insert(header::CONTENT_TYPE, mime_type.parse().unwrap());

    if let Some((start, end)) = parse_range_header(&headers, size_bytes) {
        let bytes = match state.blob_store.get_range(&r2_key, start, end).await {
            Ok(Some(payload)) => payload,
            Ok(None) => return StatusCode::NOT_FOUND.into_response(),
            Err(err) => {
                error!("Attachment ranged read failed: {}", err);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
        response_headers.insert(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, size_bytes)
                .parse()
                .unwrap(),
        );
        response_headers.insert(
            header::CONTENT_LENGTH,
            bytes.len().to_string().parse().unwrap(),
        );
        return (StatusCode::PARTIAL_CONTENT, response_headers, bytes).into_response();
    }

    let bytes = match state.blob_store.get(&r2_key).await {
        Ok(Some(payload)) => payload,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Attachment read failed: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    response_headers.insert(
        header::CONTENT_LENGTH,
        bytes.len().to_string().parse().unwrap(),
    );
    (response_headers, bytes).into_response()
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

/// GET /api/user/storage - Get current user's attachment storage usage.
async fn get_user_storage(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
) -> impl IntoResponse {
    let usage = match state.repo.get_user_storage_usage(&auth.user.id) {
        Ok(value) => value,
        Err(err) => {
            error!("Failed to query user storage usage: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let limit_bytes = match state
        .repo
        .get_effective_user_attachment_limit(&auth.user.id)
    {
        Ok(value) => value,
        Err(err) => {
            error!("Failed to query user attachment limit: {}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(UserStorageResponse {
        used_bytes: usage.used_bytes,
        blob_count: usage.blob_count,
        limit_bytes: Some(limit_bytes),
        warning_threshold: 0.8,
        over_limit: usage.used_bytes > limit_bytes,
        scope: "attachments".to_string(),
    })
    .into_response()
}

/// GET /api/workspaces/:workspace_id/history - Get git commit history
async fn get_workspace_history(
    State(state): State<ApiState>,
    RequireAuth(auth): RequireAuth,
    axum::extract::Path(workspace_id): axum::extract::Path<String>,
    Query(query): Query<HistoryQuery>,
) -> impl IntoResponse {
    // Rate limit: 60 per minute
    if let Err(retry_after) =
        state
            .rate_limiter
            .check(&auth.user.id, "history", 60, Duration::from_secs(60))
    {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
        return (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            "Rate limit exceeded",
        )
            .into_response();
    }

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

#[derive(Debug, Deserialize)]
struct SetTierRequest {
    tier: String,
}

/// PUT /api/admin/users/:user_id/tier - Set user tier (admin only)
async fn set_user_tier(
    State(state): State<ApiState>,
    headers: HeaderMap,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(body): Json<SetTierRequest>,
) -> impl IntoResponse {
    let secret = match &state.admin_secret {
        Some(s) => s,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let provided = headers
        .get("x-admin-secret")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided != secret {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let tier = match body.tier.as_str() {
        "free" => crate::db::UserTier::Free,
        "plus" => crate::db::UserTier::Plus,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Invalid tier (must be 'free' or 'plus')",
            )
                .into_response();
        }
    };

    match state.repo.set_user_tier(&user_id, tier) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            error!("Failed to set user tier: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
