//! REST server for `diaryx edit`.
//!
//! Exposes the `diaryx_core` Command/Response API over HTTP so the web app
//! can drive a local workspace without needing the CRDT sync layer.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::{Any, CorsLayer};

use diaryx_core::diaryx::Diaryx;
use diaryx_core::fs::{AsyncFileSystem, RealFileSystem, SyncToAsyncFs};
use diaryx_core::{Command, Response};

type SharedDiaryx = Arc<Diaryx<SyncToAsyncFs<RealFileSystem>>>;

struct AppState {
    diaryx: SharedDiaryx,
    workspace_root: PathBuf,
}

/// Build the axum router for the edit REST server.
pub fn edit_router(workspace_root: PathBuf) -> Router {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let diaryx = Diaryx::new(fs);
    diaryx.set_workspace_root(workspace_root.clone());

    let state = Arc::new(AppState {
        diaryx: Arc::new(diaryx),
        workspace_root,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/execute", post(handle_execute))
        .route(
            "/api/binary/{*path}",
            get(handle_read_binary).post(handle_write_binary),
        )
        .route("/api/workspace", get(handle_workspace_info))
        .with_state(state)
        .layer(cors)
}

/// POST /api/execute — run a Command, return a Response.
async fn handle_execute(
    State(state): State<Arc<AppState>>,
    Json(command): Json<Command>,
) -> Result<Json<Response>, (StatusCode, String)> {
    state
        .diaryx
        .execute(command)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// GET /api/binary/*path — read a binary file (attachments, images, etc.)
async fn handle_read_binary(
    State(state): State<Arc<AppState>>,
    Path(rel_path): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let full = state.workspace_root.join(&rel_path);
    let data = state
        .diaryx
        .fs()
        .read_binary(&full)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

    // Guess content type from extension
    let content_type = mime_from_ext(&rel_path);
    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], data))
}

/// POST /api/binary/*path — write a binary file.
async fn handle_write_binary(
    State(state): State<Arc<AppState>>,
    Path(rel_path): Path<String>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let full = state.workspace_root.join(&rel_path);
    state
        .diaryx
        .fs()
        .write_binary(&full, &body)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/workspace — return workspace metadata the frontend needs at init.
async fn handle_workspace_info(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let name = state
        .workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();

    Json(serde_json::json!({
        "workspace_path": name,
        "workspace_name": name,
    }))
}

fn mime_from_ext(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wav" => "audio/wav",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}
