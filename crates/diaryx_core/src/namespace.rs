//! Platform-agnostic server-namespace management.
//!
//! Thin wrappers around the server's `/namespaces` endpoints that take any
//! [`AuthenticatedClient`] implementation, so CLI, Tauri, and Web all drive
//! the same code paths for namespace metadata, audience/subdomain/domain/
//! subscriber CRUD, and deletion instead of open-coding the HTTP call in
//! each platform.
//!
//! ## Wire shapes
//!
//! Every type in this module serializes directly to the JSON the sync server
//! returns (see `crates/diaryx_sync_server/src/handlers/namespaces.rs`). When
//! adding new endpoints, prefer `#[serde(default)]` + optional fields so
//! we're resilient to server forward-compat additions.

use serde::{Deserialize, Serialize};

use crate::auth::{AuthError, AuthenticatedClient};

// ============================================================================
// Wire types
// ============================================================================

/// Server-side namespace metadata.
///
/// Mirrors the shape returned by `GET /namespaces/{id}`; only the fields
/// cross-platform callers actually need are modelled here (extra fields on
/// the wire are silently ignored by serde).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMetadata {
    /// Server-assigned namespace id.
    pub id: String,
    /// User id that owns this namespace.
    pub owner_user_id: String,
    /// Unix-epoch seconds when the namespace was created.
    pub created_at: i64,
    /// Arbitrary metadata attached by the creator (e.g. `{ "name": "..." }`).
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

impl NamespaceMetadata {
    /// Best-effort display name from the `metadata.name` field.
    pub fn display_name(&self) -> Option<&str> {
        self.metadata
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
    }
}

/// Audience visibility/permission entry attached to a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudienceInfo {
    /// Audience tag (matches the frontmatter `audience` values).
    pub name: String,
    /// Access mode — opaque string (e.g. `"public"`, `"private"`, `"authenticated"`).
    pub access: String,
}

/// Subdomain claim result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubdomainInfo {
    /// Claimed subdomain label (the part before the site domain).
    pub subdomain: String,
    /// Namespace id this subdomain routes to.
    pub namespace_id: String,
}

/// Custom domain registration info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainInfo {
    /// Fully-qualified custom domain.
    pub domain: String,
    /// Namespace id this domain routes to.
    pub namespace_id: String,
    /// Audience name the domain serves.
    pub audience_name: String,
    /// Unix-epoch seconds when the domain was registered.
    pub created_at: i64,
    /// Whether the server has verified domain ownership (typically via DNS).
    pub verified: bool,
}

/// Short-lived audience access token (returned by
/// `GET /namespaces/{id}/audiences/{name}/token`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResult {
    /// Opaque token string, to be appended to published-site URLs.
    pub token: String,
}

/// Returned by `POST /namespaces/{id}/audiences/{name}/rotate-password`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotatePasswordResult {
    /// The password gate's new version number after rotation. Old unlock
    /// tokens minted under any previous version are invalidated.
    pub version: u32,
    /// A fresh unlock token bound to the new version, useful for the writer
    /// to test the new password without going through the reader flow.
    pub token: String,
}

/// Subscriber (audience member) record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriberInfo {
    /// Server-assigned subscriber id.
    pub id: String,
    /// Subscriber email address.
    pub email: String,
}

/// Result of a bulk-email subscriber import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkImportResult {
    /// How many subscribers were added successfully.
    pub added: u32,
    /// Per-email error messages for rejected rows (empty on full success).
    #[serde(default)]
    pub errors: Vec<String>,
}

// ============================================================================
// URL helpers
// ============================================================================

fn namespace_path(id: &str) -> String {
    format!("/namespaces/{}", urlencoding::encode(id))
}

fn audience_path(id: &str, name: &str) -> String {
    format!(
        "/namespaces/{}/audiences/{}",
        urlencoding::encode(id),
        urlencoding::encode(name)
    )
}

fn audience_token_path(id: &str, name: &str) -> String {
    format!("{}/token", audience_path(id, name))
}

fn audience_rotate_password_path(id: &str, name: &str) -> String {
    format!("{}/rotate-password", audience_path(id, name))
}

fn audiences_path(id: &str) -> String {
    format!("/namespaces/{}/audiences", urlencoding::encode(id))
}

fn subdomain_path(id: &str) -> String {
    format!("/namespaces/{}/subdomain", urlencoding::encode(id))
}

fn domain_path(id: &str, domain: &str) -> String {
    format!(
        "/namespaces/{}/domains/{}",
        urlencoding::encode(id),
        urlencoding::encode(domain)
    )
}

fn domains_path(id: &str) -> String {
    format!("/namespaces/{}/domains", urlencoding::encode(id))
}

fn subscribers_path(id: &str, audience: &str) -> String {
    format!("{}/subscribers", audience_path(id, audience))
}

fn subscriber_path(id: &str, audience: &str, contact_id: &str) -> String {
    format!(
        "{}/{}",
        subscribers_path(id, audience),
        urlencoding::encode(contact_id)
    )
}

fn subscribers_import_path(id: &str, audience: &str) -> String {
    format!("{}/import", subscribers_path(id, audience))
}

// ============================================================================
// Response helpers
// ============================================================================

/// Extract the server's `error` string from a JSON body (falling back to
/// `fallback`) and wrap into an [`AuthError`] tagged with the HTTP status.
fn err_from(body: &str, status: u16, fallback: &str) -> AuthError {
    let msg = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| fallback.to_string());
    AuthError::new(msg, status)
}

/// Fetch metadata for a single namespace.
///
/// Returns an [`AuthError`] with the HTTP status on non-2xx responses, so
/// UI callers can distinguish 404 (already gone) from other failure modes.
pub async fn get_namespace<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<NamespaceMetadata, AuthError> {
    let resp = client.get(&namespace_path(id)).await?;
    if !resp.is_success() {
        return Err(AuthError::new(
            format!("Namespace lookup failed: HTTP {}", resp.status),
            resp.status,
        ));
    }
    resp.json()
}

/// Delete a namespace and all of its objects on the server.
///
/// This is a destructive, irreversible operation — any client currently
/// linked to the namespace will start getting 404s on its next sync and
/// will need to re-link. Returns `Ok(())` for 204 (deleted) and 404
/// (already gone, treated as idempotent), and an error otherwise.
pub async fn delete_namespace<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<(), AuthError> {
    let resp = client.delete(&namespace_path(id)).await?;
    match resp.status {
        204 => Ok(()),
        // 404 means another client (or a previous attempt) already deleted
        // it. From the caller's perspective the end state is identical to a
        // successful delete, so don't surface an error.
        404 => Ok(()),
        other => Err(AuthError::new(
            format!("Failed to delete namespace: HTTP {other}"),
            other,
        )),
    }
}

// ============================================================================
// CRUD — namespaces
// ============================================================================

/// Create a namespace.
///
/// When `id` is `None`, the server generates one. The `metadata` value is
/// stored verbatim (commonly `{ "type": "workspace", "name": "...", ... }`).
pub async fn create_namespace<C: AuthenticatedClient>(
    client: &C,
    id: Option<&str>,
    metadata: Option<&serde_json::Value>,
) -> Result<NamespaceMetadata, AuthError> {
    let mut body = serde_json::Map::new();
    if let Some(id) = id {
        body.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    }
    if let Some(metadata) = metadata {
        body.insert("metadata".to_string(), metadata.clone());
    }
    let body_str = serde_json::Value::Object(body).to_string();

    let resp = client.post("/namespaces", Some(&body_str)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to create namespace: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Update the `metadata` field on a namespace (server replaces the whole
/// metadata blob).
pub async fn update_namespace_metadata<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<NamespaceMetadata, AuthError> {
    let body = serde_json::json!({
        "metadata": metadata,
    })
    .to_string();

    let resp = client.patch(&namespace_path(id), Some(&body)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to update namespace metadata: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

// ============================================================================
// Audiences
// ============================================================================

/// List all audience entries attached to a namespace.
pub async fn list_audiences<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<Vec<AudienceInfo>, AuthError> {
    let resp = client.get(&audiences_path(id)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to list audiences: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Create or update an audience's gate stack on the server.
///
/// `gates` is a JSON array matching `diaryx_server::domain::GateInput` —
/// `[{"kind":"link"}, {"kind":"password","password":"..."}, ...]`. An empty
/// array means the audience is public. The legacy `access`-string overload
/// is kept below for back-compat.
pub async fn set_audience_gates<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    name: &str,
    gates: &serde_json::Value,
) -> Result<(), AuthError> {
    let body = serde_json::json!({ "gates": gates }).to_string();
    let resp = client.put(&audience_path(id, name), Some(&body)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to set audience: HTTP {}", resp.status),
        ));
    }
    Ok(())
}

/// Legacy access-string overload that translates the old vocabulary into
/// the new gate stack. Kept so the older UI flows keep working until they
/// are migrated to call `set_audience_gates` directly.
pub async fn set_audience<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    name: &str,
    access: &str,
) -> Result<(), AuthError> {
    let gates = match access {
        "public" => serde_json::json!([]),
        "token" => serde_json::json!([{ "kind": "link" }]),
        // Anything else (legacy `private`, unrecognized) translates to no
        // gates so the server doesn't reject the request outright.
        _ => serde_json::json!([]),
    };
    set_audience_gates(client, id, name, &gates).await
}

/// Request a short-lived access token for a specific audience. Fails with a
/// `400`-ish error if the audience does not have a `link` gate.
pub async fn get_audience_token<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    name: &str,
) -> Result<TokenResult, AuthError> {
    let resp = client.get(&audience_token_path(id, name)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to get audience token: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Rotate the password on an audience's password gate. Returns the new
/// version. Old unlock tokens minted under the previous version stop
/// validating immediately; link tokens (if the audience also has a link
/// gate) are unaffected because they don't carry a password version.
pub async fn rotate_audience_password<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    name: &str,
    password: &str,
) -> Result<RotatePasswordResult, AuthError> {
    let body = serde_json::json!({ "password": password }).to_string();
    let resp = client
        .post(&audience_rotate_password_path(id, name), Some(&body))
        .await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to rotate audience password: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

// ============================================================================
// Subdomain
// ============================================================================

/// Claim (or update) the subdomain that routes to this namespace's published
/// sites. `default_audience`, when set, determines which audience is served
/// at the subdomain root.
pub async fn claim_subdomain<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    subdomain: &str,
    default_audience: Option<&str>,
) -> Result<SubdomainInfo, AuthError> {
    let mut body = serde_json::Map::new();
    body.insert(
        "subdomain".to_string(),
        serde_json::Value::String(subdomain.to_string()),
    );
    if let Some(audience) = default_audience {
        body.insert(
            "default_audience".to_string(),
            serde_json::Value::String(audience.to_string()),
        );
    }
    let body_str = serde_json::Value::Object(body).to_string();

    let resp = client.put(&subdomain_path(id), Some(&body_str)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to claim subdomain: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Release the subdomain currently associated with a namespace.
pub async fn release_subdomain<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<(), AuthError> {
    let resp = client.delete(&subdomain_path(id)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to release subdomain: HTTP {}", resp.status),
        ));
    }
    Ok(())
}

// ============================================================================
// Custom domains
// ============================================================================

/// List custom domains registered against a namespace.
pub async fn list_domains<C: AuthenticatedClient>(
    client: &C,
    id: &str,
) -> Result<Vec<DomainInfo>, AuthError> {
    let resp = client.get(&domains_path(id)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to list domains: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Register a custom domain against a namespace audience.
pub async fn register_domain<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    domain: &str,
    audience_name: &str,
) -> Result<DomainInfo, AuthError> {
    let body = serde_json::json!({ "audience_name": audience_name }).to_string();
    let resp = client.put(&domain_path(id, domain), Some(&body)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to register domain: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Remove a custom domain from a namespace.
pub async fn remove_domain<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    domain: &str,
) -> Result<(), AuthError> {
    let resp = client.delete(&domain_path(id, domain)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to remove domain: HTTP {}", resp.status),
        ));
    }
    Ok(())
}

// ============================================================================
// Subscribers
// ============================================================================

/// List all subscribers attached to a specific audience.
pub async fn list_subscribers<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    audience: &str,
) -> Result<Vec<SubscriberInfo>, AuthError> {
    let resp = client.get(&subscribers_path(id, audience)).await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to list subscribers: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Add a single subscriber to an audience.
pub async fn add_subscriber<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    audience: &str,
    email: &str,
) -> Result<SubscriberInfo, AuthError> {
    let body = serde_json::json!({ "email": email }).to_string();
    let resp = client
        .post(&subscribers_path(id, audience), Some(&body))
        .await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to add subscriber: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

/// Remove a subscriber from an audience.
pub async fn remove_subscriber<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    audience: &str,
    contact_id: &str,
) -> Result<(), AuthError> {
    let resp = client
        .delete(&subscriber_path(id, audience, contact_id))
        .await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to remove subscriber: HTTP {}", resp.status),
        ));
    }
    Ok(())
}

/// Bulk-import a list of email addresses as subscribers for an audience.
///
/// The server returns a summary of how many were added and per-email error
/// messages for any that were rejected.
pub async fn bulk_import_subscribers<C: AuthenticatedClient>(
    client: &C,
    id: &str,
    audience: &str,
    emails: &[String],
) -> Result<BulkImportResult, AuthError> {
    let body = serde_json::json!({ "emails": emails }).to_string();
    let resp = client
        .post(&subscribers_import_path(id, audience), Some(&body))
        .await?;
    if !resp.is_success() {
        return Err(err_from(
            &resp.body,
            resp.status,
            &format!("Failed to import subscribers: HTTP {}", resp.status),
        ));
    }
    resp.json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthMetadata, HttpResponse};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Call {
        method: &'static str,
        path: String,
        body: Option<String>,
    }

    struct MockClient {
        responses: Mutex<Vec<HttpResponse>>,
        last_path: Mutex<Option<String>>,
        calls: Mutex<Vec<Call>>,
    }

    impl MockClient {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                last_path: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn record_call(&self, method: &'static str, path: &str, body: Option<&str>) {
            *self.last_path.lock().unwrap() = Some(path.to_string());
            self.calls.lock().unwrap().push(Call {
                method,
                path: path.to_string(),
                body: body.map(String::from),
            });
        }

        fn last_call(&self) -> Option<Call> {
            self.calls.lock().unwrap().last().cloned()
        }

        fn next_response(&self) -> Result<HttpResponse, AuthError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(AuthError::new("No mock response", 0))
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    #[async_trait::async_trait]
    impl AuthenticatedClient for MockClient {
        fn server_url(&self) -> &str {
            "https://app.diaryx.org/api"
        }
        async fn has_session(&self) -> bool {
            true
        }
        async fn load_metadata(&self) -> Option<AuthMetadata> {
            None
        }
        async fn save_metadata(&self, _: &AuthMetadata) {}
        async fn store_session_token(&self, _: &str) {}
        async fn clear_session(&self) {}
        async fn get(&self, path: &str) -> Result<HttpResponse, AuthError> {
            self.record_call("GET", path, None);
            self.next_response()
        }
        async fn post(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.record_call("POST", path, body);
            self.next_response()
        }
        async fn put(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.record_call("PUT", path, body);
            self.next_response()
        }
        async fn patch(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
            self.record_call("PATCH", path, body);
            self.next_response()
        }
        async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError> {
            self.record_call("DELETE", path, None);
            self.next_response()
        }
        async fn get_unauth(&self, path: &str) -> Result<HttpResponse, AuthError> {
            self.record_call("GET", path, None);
            self.next_response()
        }
        async fn post_unauth(
            &self,
            path: &str,
            body: Option<&str>,
        ) -> Result<HttpResponse, AuthError> {
            self.record_call("POST", path, body);
            self.next_response()
        }
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures_lite::future::block_on(f)
    }

    #[test]
    fn delete_namespace_accepts_204() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(delete_namespace(&client, "ns-123")).expect("204 should be Ok");
        assert_eq!(
            client.last_path.lock().unwrap().as_deref(),
            Some("/namespaces/ns-123")
        );
    }

    #[test]
    fn delete_namespace_treats_404_as_idempotent() {
        // A 404 means someone else (or a retry of ours) already deleted
        // this namespace. The caller's intent ("make sure this is gone")
        // is satisfied, so we don't surface an error.
        let client = MockClient::new(vec![HttpResponse {
            status: 404,
            body: r#"{"error":"not found"}"#.to_string(),
        }]);
        block_on(delete_namespace(&client, "ns-123")).expect("404 should be Ok (idempotent)");
    }

    #[test]
    fn delete_namespace_surfaces_other_errors() {
        let client = MockClient::new(vec![HttpResponse {
            status: 500,
            body: String::new(),
        }]);
        let err = block_on(delete_namespace(&client, "ns-123"))
            .expect_err("500 should surface as an error");
        assert_eq!(err.status_code, 500);
    }

    #[test]
    fn delete_namespace_percent_encodes_ids() {
        // Ids that contain URL-reserved characters must round-trip
        // correctly to the server; the server looks up by the decoded
        // value so we encode on the way out.
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(delete_namespace(&client, "weird id/with spaces")).unwrap();
        assert_eq!(
            client.last_path.lock().unwrap().as_deref(),
            Some("/namespaces/weird%20id%2Fwith%20spaces")
        );
    }

    #[test]
    fn get_namespace_returns_metadata_on_success() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{
                "id": "ns-1",
                "owner_user_id": "user-1",
                "created_at": 1700000000,
                "metadata": { "name": "My Journal" }
            }"#
            .to_string(),
        }]);
        let ns = block_on(get_namespace(&client, "ns-1")).unwrap();
        assert_eq!(ns.id, "ns-1");
        assert_eq!(ns.display_name(), Some("My Journal"));
    }

    #[test]
    fn get_namespace_returns_err_on_non_2xx() {
        let client = MockClient::new(vec![HttpResponse {
            status: 404,
            body: r#"{"error":"not found"}"#.to_string(),
        }]);
        let err = block_on(get_namespace(&client, "ns-gone")).expect_err("404 must error");
        assert_eq!(err.status_code, 404);
    }

    // ========================================================================
    // create / update
    // ========================================================================

    #[test]
    fn create_namespace_sends_id_and_metadata() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"id":"ns-1","owner_user_id":"u-1","created_at":1}"#.to_string(),
        }]);
        let metadata = serde_json::json!({ "name": "My Journal" });
        let ns = block_on(create_namespace(&client, Some("ns-1"), Some(&metadata))).unwrap();
        assert_eq!(ns.id, "ns-1");

        let call = client.last_call().expect("call recorded");
        assert_eq!(call.method, "POST");
        assert_eq!(call.path, "/namespaces");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(body.get("id").and_then(|v| v.as_str()), Some("ns-1"));
        assert_eq!(
            body.pointer("/metadata/name").and_then(|v| v.as_str()),
            Some("My Journal")
        );
    }

    #[test]
    fn create_namespace_server_assigned_id_omits_id_field() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"id":"ns-generated","owner_user_id":"u-1","created_at":1}"#.to_string(),
        }]);
        block_on(create_namespace(&client, None, None)).unwrap();

        let body: serde_json::Value =
            serde_json::from_str(&client.last_call().unwrap().body.unwrap()).unwrap();
        assert!(body.get("id").is_none());
        assert!(body.get("metadata").is_none());
    }

    #[test]
    fn create_namespace_surfaces_server_error_message() {
        let client = MockClient::new(vec![HttpResponse {
            status: 409,
            body: r#"{"error":"id taken"}"#.to_string(),
        }]);
        let err = block_on(create_namespace(&client, Some("ns-1"), None)).unwrap_err();
        assert_eq!(err.status_code, 409);
        assert_eq!(err.message, "id taken");
    }

    #[test]
    fn update_namespace_metadata_sends_patch_with_body() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"id":"ns-1","owner_user_id":"u-1","created_at":1,"metadata":{"name":"New"}}"#
                .to_string(),
        }]);
        let metadata = serde_json::json!({ "name": "New" });
        let ns = block_on(update_namespace_metadata(&client, "ns-1", Some(&metadata))).unwrap();
        assert_eq!(ns.display_name(), Some("New"));

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "PATCH");
        assert_eq!(call.path, "/namespaces/ns-1");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(
            body.pointer("/metadata/name").and_then(|v| v.as_str()),
            Some("New")
        );
    }

    // ========================================================================
    // audiences
    // ========================================================================

    #[test]
    fn list_audiences_parses_response() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"[{"name":"public","access":"public"},{"name":"insiders","access":"authenticated"}]"#
                .to_string(),
        }]);
        let audiences = block_on(list_audiences(&client, "ns-1")).unwrap();
        assert_eq!(audiences.len(), 2);
        assert_eq!(audiences[1].name, "insiders");
        assert_eq!(audiences[1].access, "authenticated");
        assert_eq!(
            client.last_call().unwrap().path,
            "/namespaces/ns-1/audiences"
        );
    }

    #[test]
    fn set_audience_gates_sends_put_with_gates_body() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        let gates = serde_json::json!([{ "kind": "link" }]);
        block_on(set_audience_gates(&client, "ns-1", "friends", &gates)).unwrap();

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "PUT");
        assert_eq!(call.path, "/namespaces/ns-1/audiences/friends");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(body.get("gates"), Some(&gates));
    }

    #[test]
    fn set_audience_legacy_overload_translates_access_to_gates() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(set_audience(&client, "ns-1", "friends", "token")).unwrap();
        let body: serde_json::Value =
            serde_json::from_str(&client.last_call().unwrap().body.unwrap()).unwrap();
        assert_eq!(
            body.get("gates").and_then(|v| v.as_array()).unwrap().len(),
            1
        );
        assert_eq!(
            body.pointer("/gates/0/kind").and_then(|v| v.as_str()),
            Some("link")
        );
    }

    #[test]
    fn rotate_audience_password_posts_password_and_returns_version_token() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"version":3,"token":"unlock-tok"}"#.to_string(),
        }]);
        let r = block_on(rotate_audience_password(
            &client, "ns-1", "inner", "hunter2",
        ))
        .unwrap();
        assert_eq!(r.version, 3);
        assert_eq!(r.token, "unlock-tok");

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "POST");
        assert_eq!(
            call.path,
            "/namespaces/ns-1/audiences/inner/rotate-password"
        );
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(
            body.get("password").and_then(|v| v.as_str()),
            Some("hunter2")
        );
    }

    #[test]
    fn get_audience_token_returns_token() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"token":"tok-abc"}"#.to_string(),
        }]);
        let t = block_on(get_audience_token(&client, "ns-1", "friends")).unwrap();
        assert_eq!(t.token, "tok-abc");
        assert_eq!(
            client.last_call().unwrap().path,
            "/namespaces/ns-1/audiences/friends/token"
        );
    }

    // ========================================================================
    // subdomain
    // ========================================================================

    #[test]
    fn claim_subdomain_includes_default_audience_when_given() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"subdomain":"me","namespace_id":"ns-1"}"#.to_string(),
        }]);
        let info = block_on(claim_subdomain(&client, "ns-1", "me", Some("public"))).unwrap();
        assert_eq!(info.subdomain, "me");

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "PUT");
        assert_eq!(call.path, "/namespaces/ns-1/subdomain");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(body.get("subdomain").and_then(|v| v.as_str()), Some("me"));
        assert_eq!(
            body.get("default_audience").and_then(|v| v.as_str()),
            Some("public")
        );
    }

    #[test]
    fn claim_subdomain_omits_default_audience_when_none() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"subdomain":"me","namespace_id":"ns-1"}"#.to_string(),
        }]);
        block_on(claim_subdomain(&client, "ns-1", "me", None)).unwrap();
        let body: serde_json::Value =
            serde_json::from_str(&client.last_call().unwrap().body.unwrap()).unwrap();
        assert!(body.get("default_audience").is_none());
    }

    #[test]
    fn release_subdomain_issues_delete() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(release_subdomain(&client, "ns-1")).unwrap();
        let call = client.last_call().unwrap();
        assert_eq!(call.method, "DELETE");
        assert_eq!(call.path, "/namespaces/ns-1/subdomain");
    }

    // ========================================================================
    // domains
    // ========================================================================

    #[test]
    fn register_domain_sends_audience_body() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{
                "domain":"notes.example.com",
                "namespace_id":"ns-1",
                "audience_name":"public",
                "created_at":1,
                "verified":false
            }"#
            .to_string(),
        }]);
        let info = block_on(register_domain(
            &client,
            "ns-1",
            "notes.example.com",
            "public",
        ))
        .unwrap();
        assert_eq!(info.domain, "notes.example.com");
        assert!(!info.verified);

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "PUT");
        assert_eq!(call.path, "/namespaces/ns-1/domains/notes.example.com");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(
            body.get("audience_name").and_then(|v| v.as_str()),
            Some("public")
        );
    }

    // ========================================================================
    // subscribers
    // ========================================================================

    #[test]
    fn add_subscriber_posts_email() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"id":"sub-1","email":"a@b.com"}"#.to_string(),
        }]);
        let s = block_on(add_subscriber(&client, "ns-1", "friends", "a@b.com")).unwrap();
        assert_eq!(s.email, "a@b.com");

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "POST");
        assert_eq!(call.path, "/namespaces/ns-1/audiences/friends/subscribers");
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        assert_eq!(body.get("email").and_then(|v| v.as_str()), Some("a@b.com"));
    }

    #[test]
    fn remove_subscriber_encodes_contact_id() {
        let client = MockClient::new(vec![HttpResponse {
            status: 204,
            body: String::new(),
        }]);
        block_on(remove_subscriber(&client, "ns-1", "friends", "weird id")).unwrap();
        let call = client.last_call().unwrap();
        assert_eq!(call.method, "DELETE");
        assert_eq!(
            call.path,
            "/namespaces/ns-1/audiences/friends/subscribers/weird%20id"
        );
    }

    #[test]
    fn bulk_import_subscribers_sends_email_array() {
        let client = MockClient::new(vec![HttpResponse {
            status: 200,
            body: r#"{"added":2,"errors":["bad@"]}"#.to_string(),
        }]);
        let emails = vec!["a@b.com".to_string(), "c@d.com".to_string()];
        let result =
            block_on(bulk_import_subscribers(&client, "ns-1", "friends", &emails)).unwrap();
        assert_eq!(result.added, 2);
        assert_eq!(result.errors, vec!["bad@".to_string()]);

        let call = client.last_call().unwrap();
        assert_eq!(call.method, "POST");
        assert_eq!(
            call.path,
            "/namespaces/ns-1/audiences/friends/subscribers/import"
        );
        let body: serde_json::Value = serde_json::from_str(&call.body.unwrap()).unwrap();
        let emails = body.get("emails").and_then(|v| v.as_array()).unwrap();
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0].as_str(), Some("a@b.com"));
    }
}
