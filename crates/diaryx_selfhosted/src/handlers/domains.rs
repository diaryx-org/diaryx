//! Custom domain handlers — manage domain→namespace+audience mappings and
//! serve as a Caddy `forward_auth` endpoint.
//!
//! Domain registrations are also synced to Cloudflare KV (best-effort) so the
//! site-proxy worker can resolve custom domains at the edge without hitting
//! this server.

use super::require_namespace_owner;
use crate::auth::RequireAuth;
use crate::db::NamespaceRepo;
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, put},
};
use diaryx_server::audience_token::{GateKind, validate_audience_token};
use diaryx_server::domain::CustomDomainInfo as CoreCustomDomainInfo;
use diaryx_server::domain::GateRecord;
use diaryx_server::ports::{BlobStore, DomainMappingCache, NamespaceStore, ServerCoreError};
use diaryx_server::use_cases::domains::DomainService;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::warn;

/// Shared state for domain handlers.
#[derive(Clone)]
pub struct DomainState {
    pub ns_repo: Arc<NamespaceRepo>,
    pub namespace_store: Arc<dyn NamespaceStore>,
    pub domain_mapping_cache: Arc<dyn DomainMappingCache>,
    pub blob_store: Arc<dyn BlobStore>,
    pub token_signing_key: Vec<u8>,
    /// Whether subdomain/custom-domain features are available.
    pub subdomains_available: bool,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterDomainRequest {
    pub audience_name: String,
}

#[derive(Debug, Serialize)]
pub struct DomainResponse {
    pub domain: String,
    pub namespace_id: String,
    pub audience_name: String,
    pub created_at: i64,
    pub verified: bool,
}

#[derive(Debug, Deserialize)]
pub struct ClaimSubdomainRequest {
    pub subdomain: String,
    #[serde(default)]
    pub default_audience: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubdomainResponse {
    pub subdomain: String,
    pub namespace_id: String,
    pub url: String,
}

impl From<CoreCustomDomainInfo> for DomainResponse {
    fn from(value: CoreCustomDomainInfo) -> Self {
        Self {
            domain: value.domain,
            namespace_id: value.namespace_id,
            audience_name: value.audience_name,
            created_at: value.created_at,
            verified: value.verified,
        }
    }
}

fn status_for_core_error(error: &ServerCoreError) -> StatusCode {
    match error {
        ServerCoreError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        ServerCoreError::NotFound(_) => StatusCode::NOT_FOUND,
        ServerCoreError::PermissionDenied(_) => StatusCode::FORBIDDEN,
        ServerCoreError::Conflict(_) => StatusCode::CONFLICT,
        ServerCoreError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
        ServerCoreError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        ServerCoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn core_error_response(error: ServerCoreError) -> Response {
    (
        status_for_core_error(&error),
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Owner management routes (mounted under /namespaces/{ns_id})
// ---------------------------------------------------------------------------

pub fn domain_routes(state: DomainState) -> Router {
    Router::new()
        .route("/domains", get(list_domains))
        .route(
            "/domains/{domain}",
            put(register_domain).delete(remove_domain),
        )
        .route("/subdomain", put(claim_subdomain).delete(release_subdomain))
        .with_state(state)
}

/// PUT /namespaces/{ns_id}/domains/{domain} — register a custom domain.
async fn register_domain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, domain)): Path<(String, String)>,
    Json(req): Json<RegisterDomainRequest>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service
        .register_domain(&ns_id, &domain, &req.audience_name)
        .await
    {
        Ok(info) => Json(DomainResponse::from(info)).into_response(),
        Err(error) => core_error_response(error),
    }
}

/// GET /namespaces/{ns_id}/domains — list custom domains for a namespace.
async fn list_domains(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    match state.namespace_store.list_custom_domains(&ns_id).await {
        Ok(domains) => Json(
            domains
                .into_iter()
                .map(DomainResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(error) => core_error_response(error),
    }
}

/// DELETE /namespaces/{ns_id}/domains/{domain} — remove a custom domain.
async fn remove_domain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path((ns_id, domain)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service.remove_domain(&ns_id, &domain).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => core_error_response(error),
    }
}

/// PUT /namespaces/{ns_id}/subdomain — claim a subdomain for this namespace.
async fn claim_subdomain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
    Json(req): Json<ClaimSubdomainRequest>,
) -> impl IntoResponse {
    if !state.subdomains_available {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "Subdomain features require SITE_DOMAIN configuration"})),
        )
            .into_response();
    }
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service
        .claim_subdomain(&ns_id, &req.subdomain, req.default_audience.as_deref())
        .await
    {
        Ok(claimed) => Json(SubdomainResponse {
            subdomain: claimed.subdomain.clone(),
            namespace_id: claimed.namespace_id,
            url: format!("https://{}.diaryx.org", claimed.subdomain),
        })
        .into_response(),
        Err(error) => core_error_response(error),
    }
}

/// DELETE /namespaces/{ns_id}/subdomain — release the subdomain for this namespace.
async fn release_subdomain(
    State(state): State<DomainState>,
    RequireAuth(auth): RequireAuth,
    Path(ns_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_namespace_owner(&state.ns_repo, &ns_id, &auth.user.id) {
        return resp;
    }

    let service = DomainService::new(
        state.namespace_store.as_ref(),
        state.domain_mapping_cache.as_ref(),
    );
    match service.release_subdomain(&ns_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => core_error_response(error),
    }
}

// ---------------------------------------------------------------------------
// Caddy forward_auth endpoint (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DomainAuthParams {
    pub audience_token: Option<String>,
}

pub fn domain_auth_route(state: DomainState) -> Router {
    Router::new()
        .route("/domain-auth", get(domain_auth))
        .route("/domain-check", get(domain_check))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Caddy on-demand TLS `ask` endpoint (unauthenticated)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DomainCheckParams {
    pub domain: Option<String>,
}

/// GET /domain-check?domain=example.com — Caddy on-demand TLS validation.
///
/// Returns 200 if the domain is registered in the `custom_domains` table,
/// 404 otherwise. Caddy uses this to decide whether to provision a TLS
/// certificate for an incoming hostname.
async fn domain_check(
    State(state): State<DomainState>,
    Query(params): Query<DomainCheckParams>,
) -> impl IntoResponse {
    let domain = match params.domain {
        Some(d) if !d.is_empty() => d,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };

    match state.namespace_store.get_custom_domain(&domain).await {
        Ok(Some(_)) => StatusCode::OK.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            warn!("Failed to resolve domain '{}': {}", domain, error);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /domain-auth — Caddy `forward_auth` endpoint.
///
/// Reads `X-Forwarded-Host` and `X-Forwarded-Uri` headers to resolve the
/// custom domain → namespace + audience, then checks access control.
///
/// Returns 200 with the object bytes on success, or 403/404 on failure.
async fn domain_auth(
    State(state): State<DomainState>,
    headers: HeaderMap,
    Query(params): Query<DomainAuthParams>,
) -> impl IntoResponse {
    let host = match headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
    {
        Some(h) => h.to_string(),
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let uri = headers
        .get("x-forwarded-uri")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");

    // Strip leading slash for the object key.
    let key = uri.trim_start_matches('/');

    // Look up the custom domain.
    let domain_info = match state.ns_repo.get_custom_domain(&host) {
        Some(d) => d,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let ns_id = &domain_info.namespace_id;
    let audience_name = &domain_info.audience_name;

    // Look up the object.
    let meta = match state.ns_repo.get_object_meta(ns_id, key) {
        Some(m) => m,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Object must belong to the domain's audience.
    match &meta.audience {
        Some(a) if a == audience_name => {}
        _ => return StatusCode::NOT_FOUND.into_response(),
    }

    // Check audience access level.
    let audience = match state.ns_repo.get_audience(ns_id, audience_name) {
        Some(a) => a,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // Evaluate the audience's gate stack with OR semantics.
    if audience.gates.is_empty() {
        // Public — allowed.
    } else {
        let claims = params
            .audience_token
            .as_deref()
            .and_then(|t| validate_audience_token(&state.token_signing_key, t));
        let granted = audience.gates.iter().any(|gate| match gate {
            GateRecord::Link => claims.as_ref().is_some_and(|c| {
                matches!(c.gate, GateKind::Link) && c.slug == *ns_id && c.audience == *audience_name
            }),
            GateRecord::Password { version, .. } => claims.as_ref().is_some_and(|c| {
                matches!(c.gate, GateKind::Unlock)
                    && c.slug == *ns_id
                    && c.audience == *audience_name
                    && c.password_version == Some(*version)
            }),
        });
        if !granted {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    // Serve the object bytes directly.
    let rkey = meta
        .r2_key
        .unwrap_or_else(|| format!("ns/{}/{}", ns_id, key));
    match state.blob_store.get(&rkey).await {
        Ok(Some(bytes)) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                meta.mime_type
                    .parse::<axum::http::HeaderValue>()
                    .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
            )],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::{NativeDomainMappingCache, NativeNamespaceStore},
        auth::AuthUser,
        blob_store::InMemoryBlobStore,
        db::{NamespaceRepo, init_database},
    };
    use axum::{
        body::to_bytes,
        response::{IntoResponse, Response},
    };
    use chrono::{TimeZone, Utc};
    use diaryx_server::audience_token::{
        AudienceTokenClaims, GateKind as TestGateKind, create_audience_token,
    };
    use diaryx_server::{AuthSessionInfo, BlobStore, UserInfo, UserTier};
    use reqwest::Client;
    use rusqlite::{Connection, params};
    use serde_json::{Value as JsonValue, json};
    use std::sync::{Arc, Mutex};

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

    fn state(repo: Arc<NamespaceRepo>, blob_store: Arc<InMemoryBlobStore>) -> DomainState {
        DomainState {
            ns_repo: repo.clone(),
            namespace_store: Arc::new(NativeNamespaceStore::new(repo)),
            domain_mapping_cache: Arc::new(NativeDomainMappingCache::new(
                Client::new(),
                "",
                None,
                None,
            )),
            blob_store,
            token_signing_key: b"domain-signing-key".to_vec(),
            subdomains_available: true,
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
    async fn domain_routes_cover_domain_crud() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        repo.upsert_audience("workspace:alpha", "public", &[])
            .expect("seed audience");
        let state = state(repo, Arc::new(InMemoryBlobStore::new("")));

        let registered = register_domain(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "example.com".to_string())),
            Json(RegisterDomainRequest {
                audience_name: "public".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(registered.status(), StatusCode::OK);
        let registered_body = json_body(registered).await;
        assert_eq!(registered_body["domain"], "example.com");
        assert_eq!(registered_body["audience_name"], "public");

        let listed = list_domains(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        let listed_body = json_body(listed).await;
        assert_eq!(listed_body.as_array().map(Vec::len), Some(1));

        let removed = remove_domain(
            State(state.clone()),
            auth("user1"),
            Path(("workspace:alpha".to_string(), "example.com".to_string())),
        )
        .await
        .into_response();
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);

        let empty = list_domains(
            State(state),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(json_body(empty).await, json!([]));
    }

    #[tokio::test]
    async fn domain_routes_cover_subdomain_claim_and_release() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        let state = state(repo.clone(), Arc::new(InMemoryBlobStore::new("")));

        let claimed = claim_subdomain(
            State(state.clone()),
            auth("user1"),
            Path("workspace:alpha".to_string()),
            Json(ClaimSubdomainRequest {
                subdomain: "Notes-App".to_string(),
                default_audience: Some("members".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(claimed.status(), StatusCode::OK);
        let claimed_body = json_body(claimed).await;
        assert_eq!(claimed_body["subdomain"], "notes-app");
        assert_eq!(claimed_body["url"], "https://notes-app.diaryx.org");

        let stored = repo
            .get_custom_domain("notes-app.diaryx.org")
            .expect("stored subdomain");
        assert_eq!(stored.audience_name, "*");

        let released = release_subdomain(
            State(state),
            auth("user1"),
            Path("workspace:alpha".to_string()),
        )
        .await
        .into_response();
        assert_eq!(released.status(), StatusCode::NO_CONTENT);
        assert!(repo.get_custom_domain("notes-app.diaryx.org").is_none());
    }

    #[tokio::test]
    async fn domain_check_and_public_domain_auth_serve_blob_content() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        repo.upsert_audience("workspace:alpha", "public", &[])
            .expect("seed audience");
        repo.upsert_custom_domain("example.com", "workspace:alpha", "public")
            .expect("seed domain");
        repo.upsert_object(
            "workspace:alpha",
            "index.html",
            "ns/workspace:alpha/index.html",
            "text/html",
            18,
            Some("public"),
            Some("hash"),
        )
        .expect("seed object");
        let blob_store = Arc::new(InMemoryBlobStore::new(""));
        blob_store
            .put(
                "ns/workspace:alpha/index.html",
                b"<h1>Hello</h1>",
                "text/html",
                None,
            )
            .await
            .expect("seed blob");
        let state = state(repo, blob_store);

        let domain_check_response = domain_check(
            State(state.clone()),
            Query(DomainCheckParams {
                domain: Some("example.com".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(domain_check_response.status(), StatusCode::OK);

        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "example.com".parse().unwrap());
        headers.insert("x-forwarded-uri", "/index.html".parse().unwrap());

        let response = domain_auth(
            State(state),
            headers,
            Query(DomainAuthParams {
                audience_token: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/html")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(&body[..], b"<h1>Hello</h1>");
    }

    #[tokio::test]
    async fn domain_auth_requires_valid_token_for_token_audiences() {
        let repo = setup_repo(&["user1"]);
        repo.create_namespace("workspace:alpha", "user1", None)
            .expect("seed namespace");
        repo.upsert_audience(
            "workspace:alpha",
            "members",
            &[diaryx_server::GateRecord::Link],
        )
        .expect("seed audience");
        repo.upsert_custom_domain("members.example.com", "workspace:alpha", "members")
            .expect("seed domain");
        repo.upsert_object(
            "workspace:alpha",
            "page.html",
            "ns/workspace:alpha/page.html",
            "text/html",
            20,
            Some("members"),
            Some("hash"),
        )
        .expect("seed object");
        let blob_store = Arc::new(InMemoryBlobStore::new(""));
        blob_store
            .put(
                "ns/workspace:alpha/page.html",
                b"<p>Members only</p>",
                "text/html",
                None,
            )
            .await
            .expect("seed blob");
        let state = state(repo, blob_store);

        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-host", "members.example.com".parse().unwrap());
        headers.insert("x-forwarded-uri", "/page.html".parse().unwrap());

        let forbidden = domain_auth(
            State(state.clone()),
            headers.clone(),
            Query(DomainAuthParams {
                audience_token: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let token = create_audience_token(
            &state.token_signing_key,
            &AudienceTokenClaims {
                slug: "workspace:alpha".to_string(),
                audience: "members".to_string(),
                token_id: "tok-1".to_string(),
                gate: TestGateKind::Link,
                password_version: None,
                expires_at: None,
            },
        )
        .expect("signed token");
        let allowed = domain_auth(
            State(state),
            headers,
            Query(DomainAuthParams {
                audience_token: Some(token),
            }),
        )
        .await
        .into_response();
        assert_eq!(allowed.status(), StatusCode::OK);
    }
}
