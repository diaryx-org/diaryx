use crate::auth::RequireAuth;
use crate::blob_store::BlobStore;
use crate::db::{AccessTokenInfo, AuthRepo, PublishedSiteInfo};
use crate::kv_client::CloudflareKvClient;
use crate::publish::{
    PublishLock, create_signed_token, publish_workspace_to_r2, release_publish_lock,
    try_acquire_publish_lock, write_site_meta,
};
use crate::sync_v2::SyncV2State;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone)]
pub struct SitesState {
    pub repo: Arc<AuthRepo>,
    pub sync_v2: Arc<SyncV2State>,
    pub sites_store: Arc<dyn BlobStore>,
    pub attachments_store: Arc<dyn BlobStore>,
    pub token_signing_key: Vec<u8>,
    pub sites_base_url: String,
    pub publish_lock: PublishLock,
    pub kv_client: Option<Arc<CloudflareKvClient>>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct CreateSiteRequest {
    slug: String,
    auto_publish: Option<bool>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct CreateTokenRequest {
    audience: String,
    label: Option<String>,
    expires_in: Option<String>,
}

#[derive(Debug, Serialize)]
struct AudienceBuildResponse {
    name: String,
    file_count: usize,
    built_at: i64,
}

#[derive(Debug, Serialize)]
struct SiteResponse {
    id: String,
    workspace_id: String,
    slug: String,
    custom_domain: Option<String>,
    enabled: bool,
    auto_publish: bool,
    last_published_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    audiences: Vec<AudienceBuildResponse>,
}

#[derive(Debug, Deserialize)]
struct SetDomainRequest {
    domain: String,
}

#[derive(Debug, Deserialize)]
struct VerifyDomainParams {
    domain: String,
}

#[derive(Debug, Serialize)]
struct PublishResponse {
    slug: String,
    audiences: Vec<PublishAudienceResponse>,
    published_at: i64,
}

#[derive(Debug, Serialize)]
struct PublishAudienceResponse {
    name: String,
    file_count: usize,
}

#[derive(Debug, Serialize)]
struct AccessTokenResponse {
    id: String,
    audience: String,
    label: Option<String>,
    expires_at: Option<i64>,
    revoked: bool,
    created_at: i64,
}

#[derive(Debug, Serialize)]
struct CreateTokenResponse {
    id: String,
    audience: String,
    label: Option<String>,
    expires_at: Option<i64>,
    created_at: i64,
    access_url: String,
}

pub fn site_routes(state: SitesState) -> Router {
    Router::new()
        .route(
            "/workspaces/{workspace_id}/site",
            post(create_site).get(get_site).delete(delete_site),
        )
        .route(
            "/workspaces/{workspace_id}/site/publish",
            post(trigger_publish),
        )
        .route(
            "/workspaces/{workspace_id}/site/domain",
            post(set_custom_domain).delete(remove_custom_domain),
        )
        .route(
            "/workspaces/{workspace_id}/site/tokens",
            post(create_token).get(list_tokens),
        )
        .route(
            "/workspaces/{workspace_id}/site/tokens/{token_id}",
            delete(delete_token),
        )
        .with_state(state)
}

/// Unauthenticated endpoint for Caddy's on_demand_tls `ask` directive.
/// Returns 200 if the domain has a published site, 404 otherwise.
pub fn verify_domain_route(state: SitesState) -> Router {
    Router::new()
        .route("/verify-domain", get(verify_domain))
        .with_state(state)
}

async fn create_site(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
    Json(body): Json<CreateSiteRequest>,
) -> Response {
    if !slug_is_valid(&body.slug) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_slug",
            "Slug must be 3-64 chars of lowercase letters, numbers, and hyphens",
        );
    }

    let workspace = match ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        Ok(workspace) => workspace,
        Err(resp) => return resp,
    };

    match state.repo.get_site_for_workspace(&workspace.id) {
        Ok(Some(_)) => {
            return error_response(
                StatusCode::CONFLICT,
                "site_exists",
                "Workspace already has a published site",
            );
        }
        Ok(None) => {}
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to check existing site",
            );
        }
    }

    let site_limit = match state.repo.get_effective_published_site_limit(&auth.user.id) {
        Ok(limit) => limit as usize,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to check site limit",
            );
        }
    };
    let count = match state.repo.count_user_sites(&auth.user.id) {
        Ok(count) => count,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to check site limit",
            );
        }
    };
    if count >= site_limit {
        return error_response(
            StatusCode::FORBIDDEN,
            "site_limit_reached",
            "Published site limit reached for this account",
        );
    }

    let enabled = body.enabled.unwrap_or(true);
    let auto_publish = body.auto_publish.unwrap_or(true);

    let site = match state.repo.create_published_site(
        &workspace_id,
        &auth.user.id,
        &body.slug,
        enabled,
        auto_publish,
    ) {
        Ok(site) => site,
        Err(err) => {
            let msg = err.to_string().to_lowercase();
            if msg.contains("unique") {
                return error_response(
                    StatusCode::CONFLICT,
                    "slug_conflict",
                    "Slug already exists",
                );
            }
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to create site",
            );
        }
    };

    if let Err(err) = write_site_meta(
        state.repo.as_ref(),
        state.sites_store.as_ref(),
        state.attachments_store.as_ref(),
        &site,
    )
    .await
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "meta_write_failed",
            &format!("Failed to write site metadata: {}", err),
        );
    }

    (
        StatusCode::CREATED,
        Json(site_to_response(&site, Vec::new())),
    )
        .into_response()
}

async fn get_site(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    let builds = match state.repo.list_site_audience_builds(&site.id) {
        Ok(builds) => builds,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site audience builds",
            );
        }
    };

    Json(site_to_response(&site, builds)).into_response()
}

async fn delete_site(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    if !try_acquire_publish_lock(&state.publish_lock, &workspace_id).await {
        return error_response(
            StatusCode::CONFLICT,
            "publish_in_progress",
            "Site publish is currently running",
        );
    }

    let delete_prefix = format!("{}/", site.slug);
    let cleanup_result = state.sites_store.delete_by_prefix(&delete_prefix).await;
    let db_result = state.repo.delete_published_site(&site.id);

    // Clean up custom domain KV mapping if set.
    if let Some(domain) = &site.custom_domain {
        if let Some(kv) = &state.kv_client {
            let _ = kv.delete_domain_mapping(domain).await;
        }
    }

    release_publish_lock(&state.publish_lock, &workspace_id).await;

    if cleanup_result.is_err() || db_result.is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "delete_failed",
            "Failed to delete published site",
        );
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn trigger_publish(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    if !try_acquire_publish_lock(&state.publish_lock, &workspace_id).await {
        return error_response(
            StatusCode::CONFLICT,
            "publish_in_progress",
            "Site publish is currently running",
        );
    }

    let result = publish_workspace_to_r2(
        state.repo.as_ref(),
        state.sync_v2.storage_cache.as_ref(),
        state.sites_store.as_ref(),
        state.attachments_store.as_ref(),
        &workspace_id,
        &site,
    )
    .await;

    release_publish_lock(&state.publish_lock, &workspace_id).await;

    match result {
        Ok(result) => Json(PublishResponse {
            slug: result.slug,
            audiences: result
                .audiences
                .into_iter()
                .map(|aud| PublishAudienceResponse {
                    name: aud.name,
                    file_count: aud.file_count,
                })
                .collect(),
            published_at: result.published_at,
        })
        .into_response(),
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "publish_failed",
            &format!("Publish failed: {}", err),
        ),
    }
}

async fn create_token(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
    Json(body): Json<CreateTokenRequest>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    let audience = body.audience.trim().to_lowercase();
    if audience.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_audience",
            "Audience is required",
        );
    }

    let expires_at = match parse_expires_at(body.expires_in.as_deref()) {
        Ok(expires_at) => expires_at,
        Err(message) => {
            return error_response(StatusCode::BAD_REQUEST, "invalid_expires_in", message);
        }
    };

    let builds = match state.repo.list_site_audience_builds(&site.id) {
        Ok(builds) => builds,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site audience builds",
            );
        }
    };
    let mut valid_audiences: HashSet<String> = builds.into_iter().map(|b| b.audience).collect();
    valid_audiences.insert("public".to_string());
    if !valid_audiences.contains(&audience) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_audience",
            "Audience is not available for this site",
        );
    }

    let token_id =
        match state
            .repo
            .create_access_token(&site.id, &audience, body.label.as_deref(), expires_at)
        {
            Ok(token_id) => token_id,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "db_error",
                    "Failed to create token",
                );
            }
        };

    let token = match create_signed_token(
        &state.token_signing_key,
        &site.slug,
        &audience,
        &token_id,
        expires_at,
    ) {
        Ok(token) => token,
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "token_signing_failed",
                &format!("Failed to sign token: {}", err),
            );
        }
    };

    let access_url = format!(
        "{}/{}?access={}",
        state.sites_base_url.trim_end_matches('/'),
        site.slug,
        token
    );

    (
        StatusCode::CREATED,
        Json(CreateTokenResponse {
            id: token_id,
            audience,
            label: body.label,
            expires_at,
            created_at: chrono::Utc::now().timestamp(),
            access_url,
        }),
    )
        .into_response()
}

async fn list_tokens(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    let tokens = match state.repo.list_site_tokens(&site.id) {
        Ok(tokens) => tokens,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query tokens",
            );
        }
    };

    let response: Vec<AccessTokenResponse> = tokens
        .into_iter()
        .map(|token| AccessTokenResponse {
            id: token.id,
            audience: token.audience,
            label: token.label,
            expires_at: token.expires_at,
            revoked: token.revoked,
            created_at: token.created_at,
        })
        .collect();

    Json(response).into_response()
}

async fn delete_token(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path((workspace_id, token_id)): Path<(String, String)>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    let tokens = match state.repo.list_site_tokens(&site.id) {
        Ok(tokens) => tokens,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query tokens",
            );
        }
    };
    let token: Option<AccessTokenInfo> = tokens.into_iter().find(|token| token.id == token_id);
    if token.is_none() {
        return error_response(StatusCode::NOT_FOUND, "not_found", "Token not found");
    }

    if state.repo.revoke_access_token(&token_id).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            "Failed to revoke token",
        );
    }

    if let Err(err) = write_site_meta(
        state.repo.as_ref(),
        state.sites_store.as_ref(),
        state.attachments_store.as_ref(),
        &site,
    )
    .await
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "meta_write_failed",
            &format!("Failed to update site metadata: {}", err),
        );
    }

    StatusCode::NO_CONTENT.into_response()
}

fn ensure_workspace_owner(
    state: &SitesState,
    user_id: &str,
    workspace_id: &str,
) -> Result<crate::db::WorkspaceInfo, Response> {
    let workspace = match state.repo.get_workspace(workspace_id) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "Workspace not found",
            ));
        }
        Err(_) => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query workspace",
            ));
        }
    };

    if workspace.user_id != user_id {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "Workspace not found",
        ));
    }

    Ok(workspace)
}

async fn set_custom_domain(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
    Json(body): Json<SetDomainRequest>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    let domain = body.domain.trim().to_lowercase();
    if !domain_is_valid(&domain) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_domain",
            "Domain must be a valid hostname (e.g. blog.example.com or example.com)",
        );
    }

    // Check the domain is not already taken by another site.
    match state.repo.get_site_by_custom_domain(&domain) {
        Ok(Some(existing)) if existing.id != site.id => {
            return error_response(
                StatusCode::CONFLICT,
                "domain_taken",
                "This domain is already in use by another site",
            );
        }
        Ok(_) => {}
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to check domain availability",
            );
        }
    }

    // Remove old KV mapping if changing domains.
    if let Some(old_domain) = &site.custom_domain {
        if *old_domain != domain {
            if let Some(kv) = &state.kv_client {
                let _ = kv.delete_domain_mapping(old_domain).await;
            }
        }
    }

    if state
        .repo
        .set_custom_domain(&site.id, Some(&domain))
        .is_err()
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            "Failed to set custom domain",
        );
    }

    // Write domain→slug mapping to KV.
    if let Some(kv) = &state.kv_client {
        if let Err(err) = kv.put_domain_mapping(&domain, &site.slug).await {
            tracing::error!("Failed to write KV domain mapping: {}", err);
            // Don't fail the request — SQLite is the source of truth.
        }
    }

    // Re-fetch the updated site for the response.
    let updated_site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        _ => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to read updated site",
            );
        }
    };
    let builds = state
        .repo
        .list_site_audience_builds(&updated_site.id)
        .unwrap_or_default();
    Json(site_to_response(&updated_site, builds)).into_response()
}

async fn remove_custom_domain(
    State(state): State<SitesState>,
    RequireAuth(auth): RequireAuth,
    Path(workspace_id): Path<String>,
) -> Response {
    if let Err(resp) = ensure_workspace_owner(&state, &auth.user.id, &workspace_id) {
        return resp;
    }

    let site = match state.repo.get_site_for_workspace(&workspace_id) {
        Ok(Some(site)) => site,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "Site not found");
        }
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "db_error",
                "Failed to query site",
            );
        }
    };

    if let Some(domain) = &site.custom_domain {
        if let Some(kv) = &state.kv_client {
            let _ = kv.delete_domain_mapping(domain).await;
        }
    }

    if state.repo.set_custom_domain(&site.id, None).is_err() {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "db_error",
            "Failed to remove custom domain",
        );
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn verify_domain(
    State(state): State<SitesState>,
    Query(params): Query<VerifyDomainParams>,
) -> Response {
    let domain = params.domain.trim().to_lowercase();
    match state.repo.get_site_by_custom_domain(&domain) {
        Ok(Some(_)) => StatusCode::OK.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn domain_is_valid(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    // Must have at least one dot (e.g. example.com), no protocol, no path.
    if !domain.contains('.') || domain.contains('/') || domain.contains(':') {
        return false;
    }
    // Each label: alphanumeric + hyphens, not starting/ending with hyphen.
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

fn parse_expires_at(expires_in: Option<&str>) -> Result<Option<i64>, &'static str> {
    let now = chrono::Utc::now().timestamp();
    let ttl = match expires_in {
        None => return Ok(None),
        Some(raw) if raw.trim().is_empty() => return Ok(None),
        Some("10m") => Some(10 * 60),
        Some("1d") => Some(24 * 60 * 60),
        Some("7d") => Some(7 * 24 * 60 * 60),
        Some("30d") => Some(30 * 24 * 60 * 60),
        _ => return Err("expires_in must be one of: 10m, 1d, 7d, 30d, or null"),
    };

    Ok(ttl.map(|ttl| now + ttl))
}

fn slug_is_valid(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    if bytes.len() < 3 || bytes.len() > 64 {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn error_response(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: error.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}

fn site_to_response(
    site: &PublishedSiteInfo,
    builds: Vec<crate::db::SiteAudienceBuildInfo>,
) -> SiteResponse {
    SiteResponse {
        id: site.id.clone(),
        workspace_id: site.workspace_id.clone(),
        slug: site.slug.clone(),
        custom_domain: site.custom_domain.clone(),
        enabled: site.enabled,
        auto_publish: site.auto_publish,
        last_published_at: site.last_published_at,
        created_at: site.created_at,
        updated_at: site.updated_at,
        audiences: builds
            .into_iter()
            .map(|build| AudienceBuildResponse {
                name: build.audience,
                file_count: build.file_count,
                built_at: build.built_at,
            })
            .collect(),
    }
}
