//! REST server for `diaryx edit`.
//!
//! Exposes the `diaryx_core` Command/Response API over HTTP so the web app
//! can drive a local workspace without needing the CRDT sync layer.
//!
//! Non-API requests are proxied to the upstream web app (default:
//! `https://app.diaryx.org`) so the browser sees a single `http://localhost`
//! origin — avoiding mixed-content blocks from HTTPS → HTTP.
//!
//! When the `plugins` feature is enabled, workspace plugins are loaded
//! natively at startup (same Extism runtime as Tauri) and exposed via
//! dedicated HTTP endpoints for install, uninstall, inspect, render, and
//! component-HTML retrieval.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use diaryx_core::diaryx::Diaryx;
use diaryx_core::fs::{AsyncFileSystem, RealFileSystem, SyncToAsyncFs};
use diaryx_core::{Command, Response};

#[cfg(feature = "plugins")]
use std::collections::HashMap;
#[cfg(feature = "plugins")]
use std::sync::RwLock;

#[cfg(feature = "plugins")]
use diaryx_extism::ExtismPluginAdapter;

type SharedDiaryx = Arc<Diaryx<SyncToAsyncFs<RealFileSystem>>>;

struct AppState {
    diaryx: SharedDiaryx,
    workspace_root: PathBuf,
    /// Upstream web app URL to proxy (e.g. "https://app.diaryx.org").
    upstream_url: String,
    /// Loaded plugin adapters for render/component-HTML calls.
    /// Wrapped in RwLock so install/uninstall can mutate concurrently.
    #[cfg(feature = "plugins")]
    plugin_adapters: RwLock<HashMap<String, Arc<ExtismPluginAdapter>>>,
}

/// Build the axum router for the edit REST server.
pub fn edit_router(workspace_root: PathBuf, upstream_url: String) -> Router {
    let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let mut diaryx = Diaryx::new(fs);
    diaryx.set_workspace_root(workspace_root.clone());

    // Load workspace plugins natively when the `plugins` feature is enabled.
    #[cfg(feature = "plugins")]
    let plugin_adapters = {
        let adapters =
            super::plugin_loader::register_edit_server_plugins(&mut diaryx, &workspace_root);
        if !adapters.is_empty() {
            eprintln!("[edit-server] Loaded {} plugin(s)", adapters.len(),);
        }
        adapters
    };

    let state = Arc::new(AppState {
        diaryx: Arc::new(diaryx),
        workspace_root,
        upstream_url: upstream_url.trim_end_matches('/').to_string(),
        #[cfg(feature = "plugins")]
        plugin_adapters: RwLock::new(plugin_adapters),
    });

    let mut router = Router::new()
        .route("/api/execute", post(handle_execute))
        .route(
            "/api/binary/{*path}",
            get(handle_read_binary).post(handle_write_binary),
        )
        .route("/api/workspace", get(handle_workspace_info));

    // Plugin API endpoints — only when plugins feature is enabled.
    #[cfg(feature = "plugins")]
    {
        router = router
            .route("/api/plugins/install", post(handle_plugin_install))
            .route("/api/plugins/inspect", post(handle_plugin_inspect))
            .route(
                "/api/plugins/{plugin_id}",
                axum::routing::delete(handle_plugin_uninstall),
            )
            .route(
                "/api/plugins/{plugin_id}/render",
                post(handle_plugin_render),
            )
            .route(
                "/api/plugins/{plugin_id}/component/{component_id}",
                get(handle_plugin_component_html),
            );
    }

    // Catch-all: proxy everything else to the upstream web app.
    router = router.fallback(handle_proxy);

    // When plugins are NOT available, add COOP/COEP headers so the browser's
    // Extism worker fallback can use SharedArrayBuffer.
    #[cfg(not(feature = "plugins"))]
    {
        router = router.layer(axum::middleware::from_fn(
            add_cross_origin_isolation_headers,
        ));
    }

    router.with_state(state)
}

// ============================================================================
// Core API handlers
// ============================================================================

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

    let abs_path = state.workspace_root.to_string_lossy().to_string();

    Json(serde_json::json!({
        "workspace_path": abs_path,
        "workspace_name": name,
        "native_plugins": cfg!(feature = "plugins"),
    }))
}

// ============================================================================
// Plugin API handlers (feature = "plugins")
// ============================================================================

/// POST /api/plugins/install — install a plugin from WASM bytes.
///
/// Accepts raw WASM bytes, writes to the workspace plugin directory,
/// loads the plugin, and returns the manifest JSON.
#[cfg(feature = "plugins")]
async fn handle_plugin_install(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let workspace_root = state.workspace_root.clone();
    let wasm_bytes = body.to_vec();

    // Inspect the WASM to extract the manifest (blocking — Extism is sync).
    let manifest = tokio::task::spawn_blocking(move || {
        // Write to a temp file for inspection.
        let tmp_dir =
            tempfile::tempdir().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let tmp_wasm = tmp_dir.path().join("plugin.wasm");
        std::fs::write(&tmp_wasm, &wasm_bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let guest_manifest = diaryx_extism::inspect_plugin_wasm_manifest(&tmp_wasm)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid plugin: {e}")))?;

        let plugin_id = &guest_manifest.id;

        // Write to the workspace plugin directory.
        let dest_dir = workspace_root
            .join(".diaryx")
            .join("plugins")
            .join(plugin_id);
        std::fs::create_dir_all(&dest_dir)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let dest_wasm = dest_dir.join("plugin.wasm");
        std::fs::copy(&tmp_wasm, &dest_wasm)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // Cache the manifest.
        let manifest_json = serde_json::to_string_pretty(&guest_manifest)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        std::fs::write(dest_dir.join("manifest.json"), &manifest_json)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok::<(String, String), (StatusCode, String)>((plugin_id.clone(), manifest_json))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    let (plugin_id, manifest_json) = manifest;

    // Load the plugin into the adapter map.
    let ws_root = state.workspace_root.clone();
    let pid = plugin_id.clone();
    let adapter = tokio::task::spawn_blocking(move || {
        super::plugin_loader::load_and_init_plugin(&ws_root, &pid)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    state
        .plugin_adapters
        .write()
        .unwrap()
        .insert(plugin_id, adapter);

    Ok((
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/json".to_string(),
        )],
        manifest_json,
    ))
}

/// DELETE /api/plugins/{plugin_id} — uninstall a plugin.
#[cfg(feature = "plugins")]
async fn handle_plugin_uninstall(
    State(state): State<Arc<AppState>>,
    Path(plugin_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let plugin_dir = state
        .workspace_root
        .join(".diaryx")
        .join("plugins")
        .join(&plugin_id);

    if plugin_dir.exists() {
        std::fs::remove_dir_all(&plugin_dir)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // Remove from the adapter map.
    state.plugin_adapters.write().unwrap().remove(&plugin_id);

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/plugins/inspect — inspect a WASM plugin without installing.
///
/// Returns `{ "plugin_id", "plugin_name", "requested_permissions" }`.
#[cfg(feature = "plugins")]
async fn handle_plugin_inspect(
    State(_state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let wasm_bytes = body.to_vec();

    let result = tokio::task::spawn_blocking(move || {
        let tmp_dir =
            tempfile::tempdir().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let tmp_wasm = tmp_dir.path().join("plugin.wasm");
        std::fs::write(&tmp_wasm, &wasm_bytes)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        let guest = diaryx_extism::inspect_plugin_wasm_manifest(&tmp_wasm)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid plugin: {e}")))?;

        Ok::<serde_json::Value, (StatusCode, String)>(serde_json::json!({
            "plugin_id": guest.id,
            "plugin_name": guest.name,
            "requested_permissions": guest.requested_permissions,
        }))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    Ok(Json(result))
}

/// POST /api/plugins/{plugin_id}/render — call a plugin's render export.
///
/// Expects JSON body: `{ "export_name": "render_math", "input": "..." }`.
#[cfg(feature = "plugins")]
async fn handle_plugin_render(
    State(state): State<Arc<AppState>>,
    Path(plugin_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<String, (StatusCode, String)> {
    let export_name = body
        .get("export_name")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "Missing export_name".to_string()))?
        .to_string();
    let input = body
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let adapter = {
        let adapters = state.plugin_adapters.read().unwrap();
        adapters.get(&plugin_id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Plugin '{plugin_id}' not loaded"),
            )
        })?
    };

    let result = tokio::task::spawn_blocking(move || {
        adapter
            .call_guest(&export_name, &input)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;

    Ok(result)
}

/// GET /api/plugins/{plugin_id}/component/{component_id} — get component HTML.
#[cfg(feature = "plugins")]
async fn handle_plugin_component_html(
    State(state): State<Arc<AppState>>,
    Path((plugin_id, component_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let adapter = {
        let adapters = state.plugin_adapters.read().unwrap();
        adapters.get(&plugin_id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Plugin '{plugin_id}' not loaded"),
            )
        })?
    };

    let cid = component_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        // Try dedicated export first, then fall back to handle_command.
        match adapter.call_guest("get_component_html", &cid) {
            Ok(html) => Ok(html),
            Err(_) => {
                let cmd = serde_json::json!({
                    "command": "get_component_html",
                    "params": { "component_id": cid },
                });
                let resp_str = adapter
                    .call_guest("handle_command", &cmd.to_string())
                    .map_err(|e| e.to_string())?;
                let resp: serde_json::Value =
                    serde_json::from_str(&resp_str).map_err(|e| e.to_string())?;
                resp.get("data")
                    .and_then(|d| d.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| "No data in component HTML response".to_string())
            }
        }
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        result,
    ))
}

// ============================================================================
// Proxy handler
// ============================================================================

/// Proxy non-API requests to the upstream web app.
///
/// This lets us serve the SPA from `http://localhost:PORT` so the browser
/// doesn't block fetch() calls to our local API as mixed content.
async fn handle_proxy(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let path = request.uri().path();
    let query = request.uri().query();
    let upstream = if let Some(q) = query {
        format!(
            "{}/{}?{}",
            state.upstream_url,
            path.trim_start_matches('/'),
            q
        )
    } else {
        format!("{}/{}", state.upstream_url, path.trim_start_matches('/'))
    };

    // Fetch from upstream using ureq (blocking, but fine for proxying static assets)
    let mut resp = tokio::task::spawn_blocking(move || ureq::get(&upstream).call())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", e)))?;

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let body = resp
        .body_mut()
        .read_to_vec()
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Read error: {}", e)))?;

    Ok((
        status,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        body,
    ))
}

// ============================================================================
// COOP/COEP fallback (when plugins feature is off)
// ============================================================================

/// Middleware that adds COOP + COEP headers so `window.crossOriginIsolated`
/// is `true`, enabling SharedArrayBuffer for the browser Extism worker fallback.
///
/// Only compiled when the `plugins` feature is off — with native plugins the
/// browser doesn't need cross-origin isolation.
#[cfg(not(feature = "plugins"))]
async fn add_cross_origin_isolation_headers(
    request: Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("cross-origin-opener-policy", "same-origin".parse().unwrap());
    headers.insert(
        "cross-origin-embedder-policy",
        "credentialless".parse().unwrap(),
    );
    response
}

// ============================================================================
// Helpers
// ============================================================================

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
