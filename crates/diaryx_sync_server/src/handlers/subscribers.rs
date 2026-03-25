//! Subscriber management and email dispatch handlers.
//!
//! Routes (mounted under `/namespaces/{ns_id}/audiences/{audience_name}`):
//! - `POST   /subscribers`        — add a subscriber (public)
//! - `GET    /subscribers`        — list subscribers (owner only)
//! - `DELETE /subscribers/{id}`   — remove a subscriber (owner only)
//! - `POST   /subscribers/import` — bulk import (owner only)
//! - `POST   /send-email`         — send email draft to audience (owner only)

use crate::auth::RequireAuth;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::post,
};
use diaryx_server::ports::{
    BlobStore, EmailBroadcastService, NamespaceStore, ObjectMetaStore, ServerCoreError,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct SubscriberState {
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub blob_store: Arc<dyn BlobStore>,
    pub object_meta_store: Arc<dyn ObjectMetaStore>,
    pub email_service: Arc<dyn EmailBroadcastService>,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AddSubscriberRequest {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct BulkImportRequest {
    pub emails: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SubscriberResponse {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct BulkImportResponse {
    pub added: usize,
    pub errors: Vec<String>,
}

/// Resend audience ID mapping stored in the object store.
#[derive(Debug, Serialize, Deserialize, Default)]
struct AudienceEmailConfig {
    resend_audience_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendEmailRequest {
    /// Email subject line.
    pub subject: String,
    /// Optional reply-to email address.
    #[serde(default)]
    pub reply_to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendEmailResponse {
    pub recipients: usize,
    pub send_receipt_key: String,
}

/// Send receipt stored in the object store after a successful send.
#[derive(Debug, Serialize, Deserialize)]
struct SendReceipt {
    pub timestamp: String,
    pub audience: String,
    pub recipient_count: usize,
    pub subject: String,
}

// ---------------------------------------------------------------------------
// Router (mounted under /namespaces/{ns_id}/audiences/{audience_name})
// ---------------------------------------------------------------------------

pub fn subscriber_routes(state: SubscriberState) -> Router {
    Router::new()
        .route("/subscribers", post(add_subscriber).get(list_subscribers))
        .route(
            "/subscribers/{contact_id}",
            axum::routing::delete(remove_subscriber),
        )
        .route("/subscribers/import", post(bulk_import))
        .route("/send-email", post(send_audience_email))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn core_error_response(err: ServerCoreError) -> axum::response::Response {
    let status = match &err {
        ServerCoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::json!({ "error": err.to_string() })),
    )
        .into_response()
}

/// Verify the caller owns the namespace.
async fn require_owner(
    namespace_store: &dyn NamespaceStore,
    namespace_id: &str,
    caller_user_id: &str,
) -> Result<(), ServerCoreError> {
    let ns = namespace_store
        .get_namespace(namespace_id)
        .await?
        .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;
    if ns.owner_user_id != caller_user_id {
        return Err(ServerCoreError::permission_denied(
            "You do not own this namespace",
        ));
    }
    Ok(())
}

/// Read an object's bytes by resolving the content-addressed blob via metadata.
async fn read_object_bytes(
    object_meta_store: &dyn ObjectMetaStore,
    blob_store: &dyn BlobStore,
    ns_id: &str,
    object_key: &str,
) -> Result<Option<Vec<u8>>, ServerCoreError> {
    let meta = match object_meta_store.get_object_meta(ns_id, object_key).await? {
        Some(m) => m,
        None => return Ok(None),
    };
    let blob_key = meta
        .blob_key
        .ok_or_else(|| ServerCoreError::internal("Object has no blob key"))?;
    blob_store.get(&blob_key).await
}

/// Delete an object by removing its metadata and blob.
async fn delete_object(
    object_meta_store: &dyn ObjectMetaStore,
    blob_store: &dyn BlobStore,
    ns_id: &str,
    object_key: &str,
) -> Result<(), ServerCoreError> {
    if let Some(meta) = object_meta_store.get_object_meta(ns_id, object_key).await? {
        if let Some(blob_key) = &meta.blob_key {
            let _ = blob_store.delete(blob_key).await;
        }
        object_meta_store.delete_object(ns_id, object_key).await?;
    }
    Ok(())
}

/// Object store key for the email config of an audience.
fn email_config_key(ns_id: &str, audience_name: &str) -> String {
    format!("ns/{}/_email_config/{}.json", ns_id, audience_name)
}

/// Look up the Resend audience ID for a namespace audience. Returns None if not yet created.
async fn get_resend_audience(
    blob_store: &dyn BlobStore,
    ns_id: &str,
    audience_name: &str,
) -> Result<Option<String>, ServerCoreError> {
    let key = email_config_key(ns_id, audience_name);
    if let Some(data) = blob_store.get(&key).await? {
        if let Ok(config) = serde_json::from_slice::<AudienceEmailConfig>(&data) {
            return Ok(config.resend_audience_id);
        }
    }
    Ok(None)
}

/// Get or create the Resend audience ID for a namespace audience.
async fn get_or_create_resend_audience(
    blob_store: &dyn BlobStore,
    email_service: &dyn EmailBroadcastService,
    ns_id: &str,
    audience_name: &str,
) -> Result<String, ServerCoreError> {
    let key = email_config_key(ns_id, audience_name);

    // Try to read existing config
    if let Some(data) = blob_store
        .get(&key)
        .await
        .map_err(|e| ServerCoreError::internal(e.to_string()))?
    {
        if let Ok(config) = serde_json::from_slice::<AudienceEmailConfig>(&data) {
            if let Some(id) = config.resend_audience_id {
                return Ok(id);
            }
        }
    }

    // Create a new Resend audience
    let resend_name = format!("{}/{}", ns_id, audience_name);
    let resend_id = email_service.create_audience(&resend_name).await?;

    // Save the mapping
    let config = AudienceEmailConfig {
        resend_audience_id: Some(resend_id.clone()),
    };
    let config_bytes =
        serde_json::to_vec(&config).map_err(|e| ServerCoreError::internal(e.to_string()))?;
    blob_store
        .put(&key, &config_bytes, "application/json", None)
        .await
        .map_err(|e| ServerCoreError::internal(e.to_string()))?;

    Ok(resend_id)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /namespaces/{ns_id}/audiences/{audience_name}/subscribers
///
/// Add a subscriber to an audience. This is a public endpoint (no auth required
/// for self-subscribe). Owner auth is checked separately for management endpoints.
async fn add_subscriber(
    State(state): State<SubscriberState>,
    Path((ns_id, audience_name)): Path<(String, String)>,
    Json(req): Json<AddSubscriberRequest>,
) -> impl IntoResponse {
    if !req.email.contains('@') {
        return core_error_response(ServerCoreError::invalid_input("Invalid email address"));
    }

    // Verify the namespace exists
    match state.namespace_store.get_namespace(&ns_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return core_error_response(ServerCoreError::not_found("Namespace not found"));
        }
        Err(e) => return core_error_response(e),
    }

    let resend_audience_id = match get_or_create_resend_audience(
        state.blob_store.as_ref(),
        &*state.email_service,
        &ns_id,
        &audience_name,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => return core_error_response(e),
    };

    match state
        .email_service
        .add_contact(&resend_audience_id, &req.email)
        .await
    {
        Ok(contact_id) => (
            StatusCode::CREATED,
            Json(SubscriberResponse {
                id: contact_id,
                email: req.email,
            }),
        )
            .into_response(),
        Err(e) => core_error_response(e),
    }
}

/// GET /namespaces/{ns_id}/audiences/{audience_name}/subscribers
///
/// List all subscribers for an audience. Owner only.
async fn list_subscribers(
    State(state): State<SubscriberState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, audience_name)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_owner(state.namespace_store.as_ref(), &ns_id, &auth.user.id).await {
        return core_error_response(e);
    }

    // Read-only: if no Resend audience exists yet, return empty list
    let resend_audience_id =
        match get_resend_audience(state.blob_store.as_ref(), &ns_id, &audience_name).await {
            Ok(Some(id)) => id,
            Ok(None) => return Json(Vec::<SubscriberResponse>::new()).into_response(),
            Err(e) => return core_error_response(e),
        };

    match state.email_service.list_contacts(&resend_audience_id).await {
        Ok(contacts) => {
            let response: Vec<SubscriberResponse> = contacts
                .into_iter()
                .filter(|c| !c.unsubscribed)
                .map(|c| SubscriberResponse {
                    id: c.id,
                    email: c.email,
                })
                .collect();
            Json(response).into_response()
        }
        Err(e) => core_error_response(e),
    }
}

/// DELETE /namespaces/{ns_id}/audiences/{audience_name}/subscribers/{contact_id}
///
/// Remove a subscriber from an audience. Owner only.
async fn remove_subscriber(
    State(state): State<SubscriberState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, audience_name, contact_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    if let Err(e) = require_owner(state.namespace_store.as_ref(), &ns_id, &auth.user.id).await {
        return core_error_response(e);
    }

    let resend_audience_id =
        match get_resend_audience(state.blob_store.as_ref(), &ns_id, &audience_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                return core_error_response(ServerCoreError::not_found(
                    "No subscribers configured",
                ));
            }
            Err(e) => return core_error_response(e),
        };

    match state
        .email_service
        .remove_contact(&resend_audience_id, &contact_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_error_response(e),
    }
}

/// POST /namespaces/{ns_id}/audiences/{audience_name}/subscribers/import
///
/// Bulk import subscribers. Owner only.
async fn bulk_import(
    State(state): State<SubscriberState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, audience_name)): Path<(String, String)>,
    Json(req): Json<BulkImportRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_owner(state.namespace_store.as_ref(), &ns_id, &auth.user.id).await {
        return core_error_response(e);
    }

    let resend_audience_id = match get_or_create_resend_audience(
        state.blob_store.as_ref(),
        &*state.email_service,
        &ns_id,
        &audience_name,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => return core_error_response(e),
    };

    let mut added = 0usize;
    let mut errors = Vec::new();

    for email in &req.emails {
        if !email.contains('@') {
            errors.push(format!("Invalid email: {}", email));
            continue;
        }
        match state
            .email_service
            .add_contact(&resend_audience_id, email)
            .await
        {
            Ok(_) => added += 1,
            Err(e) => errors.push(format!("{}: {}", email, e)),
        }
    }

    Json(BulkImportResponse { added, errors }).into_response()
}

/// POST /namespaces/{ns_id}/audiences/{audience_name}/send-email
///
/// Send the email draft to all active subscribers for this audience.
/// Owner only. Reads draft from `_email_draft/{audience}.html` in the object
/// store, sends via Resend batch API, writes send receipt, deletes draft.
async fn send_audience_email(
    State(state): State<SubscriberState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, audience_name)): Path<(String, String)>,
    Json(req): Json<SendEmailRequest>,
) -> impl IntoResponse {
    if let Err(e) = require_owner(state.namespace_store.as_ref(), &ns_id, &auth.user.id).await {
        return core_error_response(e);
    }

    // 1. Read email draft from the object store (content-addressed via metadata)
    let draft_object_key = format!("_email_draft/{}.html", audience_name);
    let draft_html = match read_object_bytes(
        &*state.object_meta_store,
        &*state.blob_store,
        &ns_id,
        &draft_object_key,
    )
    .await
    {
        Ok(Some(bytes)) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return core_error_response(ServerCoreError::internal(
                    "Email draft is not valid UTF-8",
                ));
            }
        },
        Ok(None) => {
            return core_error_response(ServerCoreError::not_found(
                "No email draft found. Upload a draft before sending.",
            ));
        }
        Err(e) => return core_error_response(e),
    };

    // 2. Dev mode: if email service is not configured, log and return a fake receipt
    if !state.email_service.is_configured() {
        info!(
            audience = %audience_name,
            subject = %req.subject,
            draft_bytes = draft_html.len(),
            "[Dev mode] Email send skipped — no RESEND_API_KEY. Draft content available in object store."
        );

        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let receipt = SendReceipt {
            timestamp: now.to_rfc3339(),
            audience: audience_name.clone(),
            recipient_count: 0,
            subject: req.subject.clone(),
        };
        let receipt_key = format!(
            "ns/{}/_email_log/{}/{}.json",
            ns_id, audience_name, timestamp
        );
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap_or_default();
        let _ = state
            .blob_store
            .put(&receipt_key, &receipt_bytes, "application/json", None)
            .await;

        // Delete the draft as production would
        let _ = delete_object(
            &*state.object_meta_store,
            &*state.blob_store,
            &ns_id,
            &draft_object_key,
        )
        .await;

        return (
            StatusCode::OK,
            Json(SendEmailResponse {
                recipients: 0,
                send_receipt_key: receipt_key,
            }),
        )
            .into_response();
    }

    // 3. Get Resend audience ID (must already exist to send)
    let resend_audience_id =
        match get_resend_audience(state.blob_store.as_ref(), &ns_id, &audience_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                return core_error_response(ServerCoreError::not_found(
                    "No subscribers configured for this audience",
                ));
            }
            Err(e) => return core_error_response(e),
        };

    // 4. List active contacts
    let contacts = match state.email_service.list_contacts(&resend_audience_id).await {
        Ok(c) => c,
        Err(e) => return core_error_response(e),
    };

    let active_emails: Vec<String> = contacts
        .into_iter()
        .filter(|c| !c.unsubscribed)
        .map(|c| c.email)
        .collect();

    if active_emails.is_empty() {
        return core_error_response(ServerCoreError::invalid_input(
            "No active subscribers in this audience",
        ));
    }

    // 5. Build from address from config
    let from = format!(
        "{} <{}>",
        state.email_service.from_name(),
        state.email_service.from_email()
    );

    // 6. Send via Resend batch API (chunks of 100)
    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "List-Unsubscribe".to_string(),
        "<mailto:unsubscribe@diaryx.org>".to_string(),
    );

    for chunk in active_emails.chunks(100) {
        let batch: Vec<(
            String,
            String,
            String,
            Option<String>,
            Option<std::collections::HashMap<String, String>>,
        )> = chunk
            .iter()
            .map(|email| {
                (
                    email.clone(),
                    req.subject.clone(),
                    draft_html.clone(),
                    req.reply_to.clone(),
                    Some(headers.clone()),
                )
            })
            .collect();

        if let Err(e) = state.email_service.send_batch(&from, batch).await {
            return core_error_response(e);
        }
    }

    // 7. Write send receipt
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let receipt = SendReceipt {
        timestamp: now.to_rfc3339(),
        audience: audience_name.clone(),
        recipient_count: active_emails.len(),
        subject: req.subject.clone(),
    };
    let receipt_key = format!(
        "ns/{}/_email_log/{}/{}.json",
        ns_id, audience_name, timestamp
    );
    let receipt_bytes = serde_json::to_vec(&receipt).unwrap_or_default();
    let _ = state
        .blob_store
        .put(&receipt_key, &receipt_bytes, "application/json", None)
        .await;

    // 7. Delete the draft object
    let _ = delete_object(
        &*state.object_meta_store,
        &*state.blob_store,
        &ns_id,
        &draft_object_key,
    )
    .await;

    (
        StatusCode::OK,
        Json(SendEmailResponse {
            recipients: active_emails.len(),
            send_receipt_key: receipt_key,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::NativeNamespaceStore,
        auth::AuthUser,
        blob_store::InMemoryBlobStore,
        db::{NamespaceRepo, init_database},
    };
    use axum::{
        body::to_bytes,
        response::{IntoResponse, Response},
    };
    use chrono::{TimeZone, Utc};
    use diaryx_server::domain::ContactInfo;
    use diaryx_server::{AuthSessionInfo, UserInfo, UserTier};
    use rusqlite::{Connection, params};
    use serde_json::{Value as JsonValue, json};
    use std::sync::{Arc, Mutex};

    /// Stub email service for tests — all broadcast operations return "not configured".
    struct StubEmailBroadcast;

    #[async_trait::async_trait]
    impl diaryx_server::ports::EmailBroadcastService for StubEmailBroadcast {
        fn is_configured(&self) -> bool {
            false
        }
        fn from_name(&self) -> &str {
            "Test"
        }
        fn from_email(&self) -> &str {
            "test@example.com"
        }

        async fn create_audience(&self, _name: &str) -> Result<String, ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
        async fn delete_audience(&self, _id: &str) -> Result<(), ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
        async fn add_contact(&self, _aud: &str, _email: &str) -> Result<String, ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
        async fn remove_contact(&self, _aud: &str, _contact: &str) -> Result<(), ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
        async fn list_contacts(&self, _aud: &str) -> Result<Vec<ContactInfo>, ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
        async fn send_batch(
            &self,
            _from: &str,
            _emails: Vec<(
                String,
                String,
                String,
                Option<String>,
                Option<std::collections::HashMap<String, String>>,
            )>,
        ) -> Result<(), ServerCoreError> {
            Err(ServerCoreError::unavailable("Email not configured"))
        }
    }

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

    fn test_state(repo: Arc<NamespaceRepo>, blob_store: Arc<InMemoryBlobStore>) -> SubscriberState {
        let obj_meta = Arc::new(crate::adapters::NativeObjectMetaStore::new(repo.clone()));
        SubscriberState {
            namespace_store: Arc::new(NativeNamespaceStore::new(repo)),
            blob_store,
            object_meta_store: obj_meta,
            email_service: Arc::new(StubEmailBroadcast),
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
    async fn add_subscriber_rejects_invalid_email() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        let resp = add_subscriber(
            State(state),
            Path(("ws:1".into(), "fans".into())),
            Json(AddSubscriberRequest {
                email: "not-an-email".into(),
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = json_body(resp).await;
        assert!(body["error"].as_str().unwrap().contains("Invalid email"));
    }

    #[tokio::test]
    async fn add_subscriber_rejects_missing_namespace() {
        let repo = setup_repo(&["user1"]);
        // Don't create namespace
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        let resp = add_subscriber(
            State(state),
            Path(("nonexistent".into(), "fans".into())),
            Json(AddSubscriberRequest {
                email: "test@example.com".into(),
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_subscribers_requires_ownership() {
        let repo = setup_repo(&["user1", "user2"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        // user2 tries to list user1's subscribers
        let resp = list_subscribers(
            State(state),
            auth("user2"),
            Path(("ws:1".into(), "fans".into())),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn remove_subscriber_requires_ownership() {
        let repo = setup_repo(&["user1", "user2"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        let resp = remove_subscriber(
            State(state),
            auth("user2"),
            Path(("ws:1".into(), "fans".into(), "contact-id".into())),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn bulk_import_requires_ownership() {
        let repo = setup_repo(&["user1", "user2"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        let resp = bulk_import(
            State(state),
            auth("user2"),
            Path(("ws:1".into(), "fans".into())),
            Json(BulkImportRequest {
                emails: vec!["a@b.com".into()],
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn send_email_requires_ownership() {
        let repo = setup_repo(&["user1", "user2"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let state = test_state(repo, Arc::new(InMemoryBlobStore::new("")));

        let resp = send_audience_email(
            State(state),
            auth("user2"),
            Path(("ws:1".into(), "fans".into())),
            Json(SendEmailRequest {
                subject: "Test".into(),
                reply_to: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn send_email_requires_draft() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("ws:1", "user1", None).unwrap();
        let blob_store = Arc::new(InMemoryBlobStore::new(""));
        let state = test_state(repo, blob_store);

        // No draft uploaded — should 404
        let resp = send_audience_email(
            State(state),
            auth("user1"),
            Path(("ws:1".into(), "fans".into())),
            Json(SendEmailRequest {
                subject: "Test".into(),
                reply_to: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = json_body(resp).await;
        assert!(body["error"].as_str().unwrap().contains("draft"));
    }
}
