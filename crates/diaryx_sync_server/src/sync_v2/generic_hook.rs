//! Generic namespace sync hook for the new resource backend.
//!
//! Replaces [`CloudSyncHook`] with a simpler implementation that:
//! - Authenticates via JWT + namespace ownership (not workspace listing)
//! - Authenticates guests via session code → namespace mapping
//! - No attachment reconciliation (client-driven in the new model)
//! - No file manifest handshake (clients use REST pre-sync)

use async_trait::async_trait;
use diaryx_server::sync::protocol::{AuthenticatedUser, DocType};

use super::hooks::SyncHookDelegate;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::auth::validate_token_sync;
use crate::db::{AuthRepo, NamespaceRepo, UserTier};

/// Maximum number of guests per namespace session.
const MAX_SESSION_GUESTS: usize = 5;

/// Generic namespace-based sync hook delegate.
///
/// Auth checks namespace ownership instead of workspace listing.
/// No attachment reconciliation — that's now client-driven via plugin-sync.
pub struct GenericNamespaceSyncHook {
    repo: Arc<AuthRepo>,
    ns_repo: Arc<NamespaceRepo>,
    /// Session code -> namespace ID mapping (shared with SyncV2State for peer counts).
    session_to_namespace: Arc<RwLock<HashMap<String, String>>>,
    /// Guest count per namespace (in-memory; reset to empty on server restart,
    /// which is correct because all WebSocket peers must reconnect after restart
    /// and each reconnection triggers `on_peer_joined_extra`).
    guest_counts: Arc<RwLock<HashMap<String, usize>>>,
}

impl GenericNamespaceSyncHook {
    pub fn new(
        repo: Arc<AuthRepo>,
        ns_repo: Arc<NamespaceRepo>,
        session_to_namespace: Arc<RwLock<HashMap<String, String>>>,
    ) -> Self {
        Self {
            repo,
            ns_repo,
            session_to_namespace,
            guest_counts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Authenticate via JWT token + namespace ownership.
    fn authenticate_token(
        &self,
        token: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let auth = validate_token_sync(&self.repo, token).ok_or("Invalid or expired token")?;

        let namespace_id = doc_type.workspace_id();

        // Verify user owns this namespace
        match self.ns_repo.get_namespace(namespace_id) {
            Some(ns) if ns.owner_user_id == auth.user.id => {}
            Some(_) => {
                return Err(format!("User does not own namespace '{}'", namespace_id));
            }
            None => {
                return Err(format!("Namespace '{}' not found", namespace_id));
            }
        }

        Ok(AuthenticatedUser {
            user_id: auth.user.id,
            workspace_id: namespace_id.to_string(),
            device_id: Some(auth.session.device_id),
            is_guest: false,
            read_only: false,
        })
    }

    /// Authenticate via session code (for guests).
    fn authenticate_session(
        &self,
        session_code: &str,
        guest_id: &str,
        doc_type: &DocType,
    ) -> Result<AuthenticatedUser, String> {
        let session_code = session_code.to_uppercase();

        let session = self
            .ns_repo
            .get_session(&session_code)
            .ok_or("Session not found or expired")?;

        // Verify session owner still has Plus subscription
        match self.repo.get_user_tier(&session.owner_user_id) {
            Ok(UserTier::Plus) => {}
            Ok(UserTier::Free) => return Err("Session owner's subscription has expired".into()),
            Err(e) => return Err(format!("Failed to check session owner tier: {}", e)),
        }

        // Verify this document belongs to the session's namespace
        if doc_type.workspace_id() != session.namespace_id {
            return Err("Document does not belong to session namespace".to_string());
        }

        Ok(AuthenticatedUser {
            user_id: format!("guest:{}", guest_id),
            workspace_id: session.namespace_id,
            device_id: None,
            is_guest: true,
            read_only: session.read_only,
        })
    }
}

#[async_trait]
impl SyncHookDelegate for GenericNamespaceSyncHook {
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
                Ok(user) => return Ok(user),
                Err(e) => debug!("Token auth failed: {}", e),
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
                    // Enforce guest limit per namespace
                    let count = self
                        .guest_counts
                        .read()
                        .await
                        .get(&user.workspace_id)
                        .copied()
                        .unwrap_or(0);
                    if count >= MAX_SESSION_GUESTS {
                        return Err(format!("Guest limit reached (max {})", MAX_SESSION_GUESTS));
                    }

                    info!(
                        "Authenticated guest {} for session {}",
                        guest_id, session_code,
                    );
                    self.session_to_namespace
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

        Err("Authentication required".to_string())
    }

    async fn on_peer_joined_extra(&self, doc_id: &str, user_id: &str, _peer_count: usize) {
        if user_id.starts_with("guest:") {
            if let Some(doc_type) = DocType::parse(doc_id) {
                let ns_id = doc_type.workspace_id().to_string();
                let mut counts = self.guest_counts.write().await;
                *counts.entry(ns_id).or_insert(0) += 1;
            }
        }
    }

    async fn on_peer_left_extra(&self, doc_id: &str, user_id: &str, _peer_count: usize) {
        if user_id.starts_with("guest:") {
            if let Some(doc_type) = DocType::parse(doc_id) {
                let ns_id = doc_type.workspace_id().to_string();
                let mut counts = self.guest_counts.write().await;
                if let Some(count) = counts.get_mut(&ns_id) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        counts.remove(&ns_id);
                    }
                }
            }
        }
    }
}
