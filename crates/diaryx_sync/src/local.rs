//! Local sync server for CLI-based web editing.
//!
//! Provides [`LocalSyncHook`] (a no-auth `SyncHookDelegate`) and [`start_local_server`]
//! which spins up a minimal siphonophore + axum server serving:
//!
//! - `GET /api/sessions/{code}` — returns session info for guest join
//! - `GET /sync2` — siphonophore WebSocket endpoint (upgrade to WS)
//!
//! The CLI's `diaryx edit` command uses this to enable web-based editing.

use async_trait::async_trait;
use axum::{Json, Router, extract::Path, extract::State, http::StatusCode, routing::get};
use rand::Rng;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::hooks::SyncHookDelegate;
use crate::protocol::{AuthenticatedUser, DirtyWorkspaces, DocType};
use crate::server::SyncServer;
use crate::storage::StorageCache;

// ==================== Session Code ====================

/// Generate a session code matching the format `XXXXXXXX-XXXXXXXX` (uppercase alphanumeric).
pub fn generate_session_code() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().collect();
    let mut part = || -> String {
        (0..8)
            .map(|_| chars[rng.gen_range(0..chars.len())])
            .collect()
    };
    format!("{}-{}", part(), part())
}

// ==================== LocalSyncHook ====================

/// No-auth sync hook delegate for single-workspace local editing.
///
/// Trusts all connections, validates session codes against a known code,
/// and provides a single workspace ID for all documents.
pub struct LocalSyncHook {
    workspace_id: String,
    session_code: String,
}

impl LocalSyncHook {
    pub fn new(workspace_id: String, session_code: String) -> Self {
        Self {
            workspace_id,
            session_code,
        }
    }
}

#[async_trait]
impl SyncHookDelegate for LocalSyncHook {
    async fn authenticate(
        &self,
        _doc_id: &str,
        doc_type: &DocType,
        _token: Option<&str>,
        query_params: &HashMap<String, String>,
    ) -> Result<AuthenticatedUser, String> {
        // Validate session code if provided
        if let Some(session_code) = query_params.get("session") {
            if session_code.to_uppercase() != self.session_code {
                return Err("Invalid session code".to_string());
            }
        }

        // Validate workspace access
        if doc_type.workspace_id() != self.workspace_id {
            return Err(format!(
                "Unknown workspace: {} (expected {})",
                doc_type.workspace_id(),
                self.workspace_id
            ));
        }

        Ok(AuthenticatedUser {
            user_id: "local-editor".to_string(),
            workspace_id: self.workspace_id.clone(),
            device_id: None,
            is_guest: query_params.contains_key("session"),
            read_only: false,
        })
    }

    async fn on_workspace_changed(&self, _workspace_id: &str) {
        // No-op for local mode — no git auto-commit or attachment reconciliation
    }
}

// ==================== REST Endpoints ====================

/// Shared state for the local server's REST endpoints.
#[derive(Clone)]
struct LocalServerState {
    workspace_id: String,
    session_code: String,
}

/// GET /api/sessions/{code} — returns session info for guest join.
async fn get_session(
    State(state): State<LocalServerState>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if code.to_uppercase() != state.session_code {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(serde_json::json!({
        "workspace_id": state.workspace_id,
        "read_only": false
    })))
}

// ==================== Server Startup ====================

/// Start a local sync server for web editing.
///
/// Returns the configured axum `Router` (caller binds to a port and serves).
///
/// # Arguments
/// * `workspace_root` — path to the workspace directory (for CRDT storage)
/// * `workspace_id` — unique identifier for the workspace
/// * `session_code` — the generated session code
pub fn create_local_router(
    workspace_root: PathBuf,
    workspace_id: String,
    session_code: String,
) -> Router {
    let storage_cache = Arc::new(StorageCache::new(workspace_root));
    let dirty_workspaces: DirtyWorkspaces = Arc::new(Default::default());
    let delegate = Arc::new(LocalSyncHook::new(
        workspace_id.clone(),
        session_code.clone(),
    ));

    let sync_server = SyncServer::new(delegate, storage_cache, dirty_workspaces);

    let rest_state = LocalServerState {
        workspace_id,
        session_code: session_code.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    info!(
        "Local sync server created with session code: {}",
        session_code
    );

    Router::new()
        .route("/api/sessions/{code}", get(get_session))
        .with_state(rest_state)
        .merge(sync_server.router)
        .layer(cors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_code_format() {
        let code = generate_session_code();
        assert!(
            code.len() == 17,
            "Session code should be 17 chars (8-8): {}",
            code
        );
        assert!(code.contains('-'), "Session code should contain a dash");
        let parts: Vec<&str> = code.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 8);
        assert!(
            parts[0]
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
            "Code should be uppercase alphanumeric"
        );
    }
}
