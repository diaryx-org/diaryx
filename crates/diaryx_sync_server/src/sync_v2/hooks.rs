//! Cloud sync hook implementation for the hosted Diaryx sync server.
//!
//! This module implements [`SyncHookDelegate`] for the cloud server, providing:
//! - JWT authentication and session validation
//! - Attachment reconciliation on workspace changes
//! - Session-to-workspace mapping for guest peers

use async_trait::async_trait;
use diaryx_sync::hooks::SyncHookDelegate;
use diaryx_sync::protocol::DocType;
use diaryx_sync::storage::StorageCache;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::auth::validate_token;
use crate::db::AuthRepo;

// Re-export AuthenticatedUser for use by other sync_server modules
pub use diaryx_sync::protocol::AuthenticatedUser;

/// Cloud sync hook delegate providing JWT auth and attachment reconciliation.
pub struct CloudSyncHook {
    /// Auth repository for token validation.
    repo: Arc<AuthRepo>,
    /// Shared storage cache (also used by WorkspaceStore for HTTP API operations).
    storage_cache: Arc<StorageCache>,
    /// Shared session-to-workspace mapping (also used by SyncV2State for peer counts).
    session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
    /// Debounced workspace attachment reconciliation timers.
    attachment_reconcile_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl CloudSyncHook {
    /// Create a new CloudSyncHook.
    pub fn new(
        repo: Arc<AuthRepo>,
        storage_cache: Arc<StorageCache>,
        session_to_workspace: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self {
            repo,
            storage_cache,
            session_to_workspace,
            attachment_reconcile_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Authenticate from JWT token.
    fn authenticate_token(
        &self,
        token: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let auth = validate_token(&self.repo, token).ok_or("Invalid or expired token")?;

        let workspace_id = doc_type.workspace_id();

        // Verify user has access to this workspace
        let workspaces = self
            .repo
            .get_user_workspaces(&auth.user.id)
            .unwrap_or_default();

        let has_access = workspaces
            .iter()
            .any(|w| w.id == workspace_id || w.name == workspace_id);

        if !has_access {
            return Err(format!(
                "User does not have access to workspace '{}'",
                workspace_id
            ));
        }
        let workspace_id = workspace_id.to_string();

        Ok(AuthenticatedUser {
            user_id: auth.user.id,
            workspace_id,
            device_id: Some(auth.session.device_id),
            is_guest: false,
            read_only: false,
        })
    }

    /// Authenticate from session code (for guests).
    fn authenticate_session(
        &self,
        session_code: &str,
        guest_id: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let session_code = session_code.to_uppercase();

        let session = self
            .repo
            .get_share_session(&session_code)
            .map_err(|e| format!("Failed to get session: {}", e))?
            .ok_or("Session not found")?;

        // Verify this document belongs to the session's workspace
        if doc_type.workspace_id() != session.workspace_id {
            return Err("Document does not belong to session workspace".to_string());
        }

        Ok(AuthenticatedUser {
            user_id: format!("guest:{}", guest_id),
            workspace_id: session.workspace_id,
            device_id: None,
            is_guest: true,
            read_only: session.read_only,
        })
    }

    async fn schedule_workspace_attachment_reconcile(&self, workspace_id: String) {
        let mut tasks = self.attachment_reconcile_tasks.lock().await;
        if let Some(existing) = tasks.remove(&workspace_id) {
            existing.abort();
        }

        let repo = self.repo.clone();
        let storage_cache = self.storage_cache.clone();
        let tasks_map = self.attachment_reconcile_tasks.clone();
        let task_workspace_id = workspace_id.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let store = crate::sync_v2::WorkspaceStore::new(storage_cache);
            match store.reconcile_workspace_attachment_refs(&task_workspace_id, &repo) {
                Ok(ref_count) => {
                    debug!(
                        "Attachment reconciliation complete for {} ({} refs)",
                        task_workspace_id, ref_count
                    );
                }
                Err(err) => {
                    warn!(
                        "Attachment reconciliation failed for {}: {}",
                        task_workspace_id, err
                    );
                }
            }
            let mut tasks = tasks_map.lock().await;
            tasks.remove(&task_workspace_id);
        });
        tasks.insert(workspace_id, handle);
    }
}

#[async_trait]
impl SyncHookDelegate for CloudSyncHook {
    async fn authenticate(
        &self,
        _doc_id: &str,
        doc_type: &DocType,
        token: Option<&str>,
        query_params: &HashMap<String, String>,
    ) -> Result<AuthenticatedUser, String> {
        // Try JWT token first
        if let Some(token) = token {
            match self.authenticate_token(token, doc_type) {
                Ok(user) => {
                    return Ok(user);
                }
                Err(e) => {
                    debug!("Token auth failed: {}", e);
                }
            }
        }

        // Try session code
        if let Some(session_code) = query_params.get("session") {
            let guest_id = query_params
                .get("guest_id")
                .cloned()
                .unwrap_or_else(|| format!("guest-{}", uuid::Uuid::new_v4()));

            match self.authenticate_session(session_code, &guest_id, doc_type) {
                Ok(user) => {
                    info!(
                        "Authenticated guest {} for session {}",
                        guest_id, session_code,
                    );
                    // Register session-to-workspace mapping for peer count lookups
                    self.session_to_workspace
                        .write()
                        .await
                        .insert(session_code.to_uppercase(), user.workspace_id.clone());
                    return Ok(user);
                }
                Err(e) => {
                    warn!("Session auth failed for {}: {}", session_code, e);
                    return Err(e);
                }
            }
        }

        // No valid auth method
        Err("Authentication required".to_string())
    }

    async fn on_workspace_changed(&self, workspace_id: &str) {
        self.schedule_workspace_attachment_reconcile(workspace_id.to_string())
            .await;
    }
}
