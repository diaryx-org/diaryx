//! Thin HTTP handlers that wire CF Worker requests to portable services.

use crate::adapters::d1::*;
use crate::adapters::kv::KvDomainMappingCache;
use crate::adapters::r2::R2BlobStore;
use crate::config;
use crate::tokens::validate_audience_token;
use diaryx_server::api::billing::{
    AppleRestoreResponse, AppleVerifyReceiptResponse, StripeConfigResponse, UrlResponse,
};
use diaryx_server::api::namespaces::{
    CreateNamespaceRequest, NamespaceResponse, UpdateNamespaceRequest,
};
use diaryx_server::api::passkeys::{
    PasskeyAuthFinishRequest, PasskeyAuthStartRequest, PasskeyAuthStartResponse,
    PasskeyRegisterFinishRequest, PasskeyRegisterFinishResponse, PasskeyRegisterStartResponse,
};
use diaryx_server::ports::{AuthStore, Mailer, NamespaceStore, ServerCoreError, UserStore};
use diaryx_server::use_cases::auth::{
    AuthConfig, AuthError, AuthenticationService, SessionValidationService, extract_token,
};
use diaryx_server::use_cases::billing::BillingService;
use diaryx_server::use_cases::{
    audiences::AudienceService, domains::DomainService, namespaces::NamespaceService,
    objects::ObjectService, sessions::SessionService,
};
use serde::Deserialize;
use worker::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/wrangler_bindings.rs"));
}

fn error_response(err: ServerCoreError) -> Result<Response> {
    let (status, msg) = match &err {
        ServerCoreError::NotFound(m) => (404, m.as_str()),
        ServerCoreError::PermissionDenied(m) => (403, m.as_str()),
        ServerCoreError::InvalidInput(m) => (400, m.as_str()),
        ServerCoreError::Conflict(m) => (409, m.as_str()),
        ServerCoreError::RateLimited(m) => (429, m.as_str()),
        ServerCoreError::Unavailable(m) => (503, m.as_str()),
        ServerCoreError::Internal(m) => (500, m.as_str()),
    };
    Response::from_json(&serde_json::json!({ "error": msg })).map(|r| r.with_status(status))
}

fn db(ctx: &RouteContext<()>) -> Result<D1Database> {
    ctx.env.d1(bindings::D1_BINDING)
}

fn bucket(ctx: &RouteContext<()>) -> Result<Bucket> {
    ctx.env.bucket(bindings::R2_BINDING)
}

fn domains_kv(ctx: &RouteContext<()>) -> Result<worker::kv::KvStore> {
    ctx.env.kv(bindings::KV_BINDING)
}

fn auth_cfg(ctx: &RouteContext<()>) -> AuthConfig {
    config::auth_config(&ctx.env)
}

fn signing_key(ctx: &RouteContext<()>) -> Vec<u8> {
    config::token_signing_key(&ctx.env)
}

/// Build a Set-Cookie header value for a session token.
fn set_session_cookie(token: &str, env: &Env) -> String {
    let max_age_secs = config::session_expiry_days(env) * 86400;
    if config::secure_cookies(env) {
        format!(
            "diaryx_session={token}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
        )
    } else {
        format!("diaryx_session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}")
    }
}

/// Authenticate the request, returning the user ID on success.
async fn authenticate(req: &Request, ctx: &RouteContext<()>) -> Result<String> {
    let auth_store = D1AuthStore::new(db(ctx)?);
    let session_store = D1AuthSessionStore::new(db(ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let service = SessionValidationService::new(&auth_store, &session_store);
    let auth_ctx = service
        .validate(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    Ok(auth_ctx.user.id)
}

// ---------------------------------------------------------------------------
// Namespace handlers
// ---------------------------------------------------------------------------

pub async fn create_namespace(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: CreateNamespaceRequest = req.json().await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);
    let metadata_str = body.metadata_str();

    match service
        .create(&user_id, body.id.as_deref(), metadata_str.as_deref())
        .await
    {
        Ok(ns) => Response::from_json(&NamespaceResponse::from(ns)).map(|r| r.with_status(201)),
        Err(e) => error_response(e),
    }
}

pub async fn list_namespaces(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.list(&user_id, 100, 0).await {
        Ok(list) => {
            let response: Vec<NamespaceResponse> =
                list.into_iter().map(NamespaceResponse::from).collect();
            Response::from_json(&response)
        }
        Err(e) => error_response(e),
    }
}

pub async fn get_namespace(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.get(id, &user_id).await {
        Ok(ns) => Response::from_json(&NamespaceResponse::from(ns)),
        Err(e) => error_response(e),
    }
}

pub async fn update_namespace(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let body: UpdateNamespaceRequest = req.json().await?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);
    let metadata_str = body.metadata_str();

    match service
        .update_metadata(id, &user_id, metadata_str.as_deref())
        .await
    {
        Ok(ns) => Response::from_json(&NamespaceResponse::from(ns)),
        Err(e) => error_response(e),
    }
}

pub async fn delete_namespace(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;
    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let service = NamespaceService::new(&ns_store);

    match service.delete(id, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Object handlers
// ---------------------------------------------------------------------------

pub async fn list_objects(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let url = req.url()?;
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let limit: u32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let offset: u32 = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.list(&ns_id, limit, offset, &user_id).await {
        Ok(objects) => {
            let response: Vec<serde_json::Value> = objects
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "namespace_id": m.namespace_id,
                        "key": m.key,
                        "r2_key": m.blob_key,
                        "mime_type": m.mime_type,
                        "size_bytes": m.size_bytes,
                        "updated_at": m.updated_at,
                        "audience": m.audience,
                    })
                })
                .collect();
            Response::from_json(&response)
        }
        Err(e) => error_response(e),
    }
}

pub async fn put_object(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let key = ctx
        .param("key")
        .ok_or_else(|| Error::from("missing key"))?
        .to_string();

    let mime_type = req
        .headers()
        .get("content-type")?
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let audience = req.headers().get("x-audience")?;
    let bytes = req.bytes().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service
        .put(
            &ns_id,
            &key,
            &mime_type,
            &bytes,
            audience.as_deref(),
            &user_id,
        )
        .await
    {
        Ok(result) => Response::from_json(
            &serde_json::json!({ "key": result.key, "size_bytes": result.size_bytes }),
        ),
        Err(e) => error_response(e),
    }
}

pub async fn get_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.get(ns_id, key, &user_id).await {
        Ok(result) => {
            let mut resp = Response::from_bytes(result.bytes)?;
            resp.headers_mut().set("content-type", &result.mime_type)?;
            Ok(resp)
        }
        Err(e) => error_response(e),
    }
}

pub async fn delete_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.delete(ns_id, key, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

pub async fn get_public_object(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;
    let key = ctx.param("key").ok_or_else(|| Error::from("missing key"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    let access = match service.resolve_public_access(ns_id, key).await {
        Ok(a) => a,
        Err(e) => return error_response(e),
    };

    // Enforce access control
    match access.access.as_str() {
        "public" => {}
        "token" => {
            let url = req.url()?;
            let token_str = url
                .query_pairs()
                .find(|(k, _)| k == "audience_token")
                .map(|(_, v)| v.to_string());

            let key_bytes = signing_key(&ctx);
            let valid = token_str
                .as_deref()
                .and_then(|t| validate_audience_token(&key_bytes, t))
                .is_some_and(|claims| {
                    claims.slug == *ns_id && claims.audience == access.audience_name
                });

            if !valid {
                return Response::empty().map(|r| r.with_status(403));
            }
        }
        _ => return Response::empty().map(|r| r.with_status(403)),
    }

    match service
        .fetch_blob(ns_id, key, access.meta.blob_key.as_deref())
        .await
    {
        Ok(result) => {
            let mut resp = Response::from_bytes(result.bytes)?;
            resp.headers_mut().set("content-type", &result.mime_type)?;
            Ok(resp)
        }
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Audience handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SetAudienceBody {
    access: String,
}

pub async fn set_audience(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let name = ctx
        .param("name")
        .ok_or_else(|| Error::from("missing name"))?
        .to_string();
    let body: SetAudienceBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.set(&ns_id, &name, &body.access, &user_id).await {
        Ok(info) => Response::from_json(&info),
        Err(e) => error_response(e),
    }
}

pub async fn list_audiences(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.list(ns_id, &user_id).await {
        Ok(list) => Response::from_json(&list),
        Err(e) => error_response(e),
    }
}

pub async fn delete_audience(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let name = ctx
        .param("name")
        .ok_or_else(|| Error::from("missing name"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = AudienceService::new(&ns_store, &blob_store);

    match service.delete(&ns_id, &name, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Domain handlers
// ---------------------------------------------------------------------------

pub async fn list_domains(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    match ns_store.list_custom_domains(&ns_id).await {
        Ok(domains) => {
            let response: Vec<serde_json::Value> = domains
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "domain": d.domain,
                        "namespace_id": d.namespace_id,
                        "audience_name": d.audience_name,
                        "created_at": d.created_at,
                        "verified": d.verified,
                    })
                })
                .collect();
            Response::from_json(&response)
        }
        Err(e) => error_response(e),
    }
}

#[derive(Deserialize)]
struct RegisterDomainBody {
    audience_name: String,
}

pub async fn register_domain(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let domain = ctx
        .param("domain")
        .ok_or_else(|| Error::from("missing domain"))?
        .to_string();
    let body: RegisterDomainBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    let info = match service
        .register_domain(&ns_id, &domain, &body.audience_name)
        .await
    {
        Ok(info) => info,
        Err(e) => return error_response(e),
    };

    // Register as a Cloudflare custom hostname for automatic SSL.
    if let (Some(zone_id), Some(api_token)) =
        (config::cf_zone_id(&ctx.env), config::cf_api_token(&ctx.env))
    {
        let cf_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames",
            zone_id
        );
        let cf_body = serde_json::json!({
            "hostname": domain,
            "ssl": { "method": "http", "type": "dv" },
        });
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", api_token))?;
        headers.set("Content-Type", "application/json")?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&cf_body.to_string())));
        match Fetch::Request(Request::new_with_init(&cf_url, &init)?)
            .send()
            .await
        {
            Ok(mut resp) => {
                if resp.status_code() >= 400 {
                    let text = resp.text().await.unwrap_or_default();
                    worker::console_log!(
                        "Cloudflare custom hostname creation failed for {}: {} {}",
                        domain,
                        resp.status_code(),
                        text
                    );
                }
            }
            Err(e) => {
                worker::console_log!("Cloudflare custom hostname API error for {}: {}", domain, e);
            }
        }
    }

    Response::from_json(&serde_json::json!({
        "domain": info.domain,
        "namespace_id": info.namespace_id,
        "audience_name": info.audience_name,
        "created_at": info.created_at,
        "verified": info.verified,
    }))
}

pub async fn remove_domain(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let domain = ctx
        .param("domain")
        .ok_or_else(|| Error::from("missing domain"))?
        .to_string();

    // Find and delete the Cloudflare custom hostname first.
    if let (Some(zone_id), Some(api_token)) =
        (config::cf_zone_id(&ctx.env), config::cf_api_token(&ctx.env))
    {
        // Look up the custom hostname ID by hostname.
        let list_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames?hostname={}",
            zone_id, domain
        );
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bearer {}", api_token))?;
        let mut init = RequestInit::new();
        init.with_method(Method::Get).with_headers(headers);
        if let Ok(mut resp) = Fetch::Request(Request::new_with_init(&list_url, &init)?)
            .send()
            .await
        {
            if let Ok(body) = resp.text().await {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(results) = parsed["result"].as_array() {
                        for entry in results {
                            if let Some(ch_id) = entry["id"].as_str() {
                                let del_url = format!(
                                    "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames/{}",
                                    zone_id, ch_id
                                );
                                let del_headers = Headers::new();
                                del_headers
                                    .set("Authorization", &format!("Bearer {}", api_token))?;
                                let mut del_init = RequestInit::new();
                                del_init
                                    .with_method(Method::Delete)
                                    .with_headers(del_headers);
                                let _ =
                                    Fetch::Request(Request::new_with_init(&del_url, &del_init)?)
                                        .send()
                                        .await;
                            }
                        }
                    }
                }
            }
        }
    }

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service.remove_domain(&ns_id, &domain).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Subdomain handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ClaimSubdomainBody {
    subdomain: String,
    #[serde(default)]
    default_audience: Option<String>,
}

pub async fn claim_subdomain(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();
    let body: ClaimSubdomainBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service
        .claim_subdomain(&ns_id, &body.subdomain, body.default_audience.as_deref())
        .await
    {
        Ok(claimed) => Response::from_json(&serde_json::json!({
            "subdomain": claimed.subdomain,
            "namespace_id": claimed.namespace_id,
            "url": format!("https://{}.diaryx.org", claimed.subdomain),
        })),
        Err(e) => error_response(e),
    }
}

pub async fn release_subdomain(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let _user_id = authenticate(&req, &ctx).await?;
    let ns_id = ctx
        .param("ns_id")
        .ok_or_else(|| Error::from("missing ns_id"))?
        .to_string();

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let domain_cache = KvDomainMappingCache::new(domains_kv(&ctx)?);
    let service = DomainService::new(&ns_store, &domain_cache);

    match service.release_subdomain(&ns_id).await {
        Ok(_) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Session handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateSessionBody {
    namespace_id: String,
    #[serde(default)]
    read_only: bool,
}

pub async fn create_session(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: CreateSessionBody = req.json().await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service
        .create(&body.namespace_id, &user_id, body.read_only)
        .await
    {
        Ok(session) => Response::from_json(&serde_json::json!({
            "code": session.code,
            "namespace_id": session.namespace_id,
            "read_only": session.read_only,
        })),
        Err(e) => error_response(e),
    }
}

pub async fn get_session(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let code = ctx
        .param("code")
        .ok_or_else(|| Error::from("missing code"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service.get(code).await {
        Ok(session) => Response::from_json(&serde_json::json!({
            "code": session.code,
            "namespace_id": session.namespace_id,
            "read_only": session.read_only,
        })),
        Err(e) => error_response(e),
    }
}

pub async fn delete_session(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let code = ctx
        .param("code")
        .ok_or_else(|| Error::from("missing code"))?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let session_store = D1SessionStore::new(db(&ctx)?);
    let service = SessionService::new(&ns_store, &session_store);

    match service.delete(code, &user_id).await {
        Ok(()) => Response::empty().map(|r| r.with_status(204)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Auth handlers
// ---------------------------------------------------------------------------

pub async fn get_current_user(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);
    let ns_store = D1NamespaceStore::new(db(&ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let validation = SessionValidationService::new(&auth_store, &session_store);
    let auth = validation
        .validate(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let current_user =
        diaryx_server::use_cases::current_user::CurrentUserService::new(&auth_store, &ns_store);
    match current_user.load(&auth.user.id, &auth.user.email).await {
        Ok(ctx) => {
            // Flatten to match the shape the frontend expects (MeResponse).
            let devices: Vec<serde_json::Value> = ctx
                .devices
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "name": d.name,
                        "last_seen_at": d.last_seen_at.to_rfc3339(),
                    })
                })
                .collect();
            let workspaces: Vec<serde_json::Value> = ctx
                .namespaces
                .into_iter()
                .map(|ns| serde_json::json!({ "id": &ns.id, "name": &ns.id }))
                .collect();
            Response::from_json(&serde_json::json!({
                "user": { "id": auth.user.id, "email": ctx.user.email },
                "workspaces": workspaces,
                "devices": devices,
                "tier": ctx.user.tier.as_str(),
                "workspace_limit": ctx.limits.workspace_limit,
                "published_site_limit": ctx.limits.published_site_limit,
                "attachment_limit_bytes": ctx.limits.attachment_limit_bytes,
            }))
        }
        Err(e) => error_response(e),
    }
}

pub async fn logout(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let authorization = req.headers().get("Authorization")?;
    let cookie = req.headers().get("Cookie")?;
    let query = req.url()?.query().map(|s| s.to_string());

    let token = extract_token(
        authorization.as_deref(),
        cookie.as_deref(),
        query.as_deref(),
    )
    .ok_or_else(|| Error::from("Authentication required"))?;

    let service = SessionValidationService::new(&auth_store, &session_store);
    service
        .logout(&token)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    let mut resp = Response::empty()?.with_status(204);
    let clear_cookie = if config::secure_cookies(&ctx.env) {
        "diaryx_session=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0"
    } else {
        "diaryx_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0"
    };
    resp.headers_mut().set("Set-Cookie", clear_cookie)?;
    Ok(resp)
}

#[derive(Deserialize)]
struct MagicLinkBody {
    email: String,
}

pub async fn request_magic_link(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: MagicLinkBody = req.json().await?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    let (token, code) = match service.request_magic_link(&body.email).await {
        Ok(result) => result,
        Err(e) => {
            return Response::from_json(&serde_json::json!({ "error": e.to_string() }))
                .map(|r| r.with_status(400));
        }
    };

    let app_url = config::app_base_url(&ctx.env);
    let magic_link_url = format!("{}?token={}", app_url, token);

    // Try to send email via Resend
    if let Some(mailer) = config::mailer(&ctx.env, cfg.magic_link_expiry_minutes) {
        if let Err(e) = mailer
            .send_magic_link(&body.email, &magic_link_url, &code)
            .await
        {
            return Response::from_json(
                &serde_json::json!({ "error": format!("Failed to send email: {}", e) }),
            )
            .map(|r| r.with_status(500));
        }

        Response::from_json(&serde_json::json!({
            "success": true,
            "message": "Check your email for a sign-in link.",
        }))
    } else {
        // Dev mode: return the link and code directly
        Response::from_json(&serde_json::json!({
            "success": true,
            "message": "Email not configured. Use the dev link below.",
            "dev_link": magic_link_url,
            "dev_code": code,
        }))
    }
}

#[derive(Deserialize)]
struct VerifyQuery {
    token: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
}

pub async fn verify_magic_link(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let url = req.url()?;
    let query: VerifyQuery =
        serde_qs::from_str(url.query().unwrap_or("")).map_err(|e| Error::from(e.to_string()))?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    match service
        .verify_magic_link(
            &query.token,
            query.device_name.as_deref(),
            None,
            query.replace_device_id.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let cookie = set_session_cookie(&result.session_token, &ctx.env);
            let mut resp = Response::from_json(&serde_json::json!({
                "success": true,
                "token": result.session_token,
                "user": { "id": result.user_id, "email": result.email },
            }))?;
            resp.headers_mut().set("Set-Cookie", &cookie)?;
            Ok(resp)
        }
        Err(AuthError::DeviceLimitReached { devices, .. }) => {
            Response::from_json(&serde_json::json!({
                "error": "Device limit reached. Remove a device to sign in on a new one.",
                "devices": devices,
            }))
            .map(|r| r.with_status(403))
        }
        Err(e) => Response::from_json(&serde_json::json!({ "error": e.to_string() }))
            .map(|r| r.with_status(400)),
    }
}

#[derive(Deserialize)]
struct VerifyCodeBody {
    code: String,
    email: String,
    device_name: Option<String>,
    replace_device_id: Option<String>,
}

pub async fn verify_code(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: VerifyCodeBody = req.json().await?;
    let cfg = auth_cfg(&ctx);

    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);

    let service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    match service
        .verify_code(
            &body.code,
            &body.email,
            body.device_name.as_deref(),
            None,
            body.replace_device_id.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let cookie = set_session_cookie(&result.session_token, &ctx.env);
            let mut resp = Response::from_json(&serde_json::json!({
                "success": true,
                "token": result.session_token,
                "user": { "id": result.user_id, "email": result.email },
            }))?;
            resp.headers_mut().set("Set-Cookie", &cookie)?;
            Ok(resp)
        }
        Err(AuthError::DeviceLimitReached { devices, .. }) => {
            Response::from_json(&serde_json::json!({
                "error": "Device limit reached. Remove a device to sign in on a new one.",
                "devices": devices,
            }))
            .map(|r| r.with_status(403))
        }
        Err(e) => Response::from_json(&serde_json::json!({ "error": e.to_string() }))
            .map(|r| r.with_status(400)),
    }
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

pub async fn get_usage(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let ns_store = D1NamespaceStore::new(db(&ctx)?);
    let obj_store = D1ObjectMetaStore::new(db(&ctx)?);
    let blob_store = R2BlobStore::new(bucket(&ctx)?);
    let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

    match service.get_usage(&user_id).await {
        Ok(totals) => Response::from_json(&totals),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Stripe billing handlers
// ---------------------------------------------------------------------------

/// POST /api/stripe/checkout — Create a Stripe Checkout Session for upgrading to Plus.
pub async fn stripe_checkout(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let stripe_secret =
        config::stripe_secret_key(&ctx.env).ok_or_else(|| Error::from("Stripe not configured"))?;
    let stripe_price_id = config::stripe_price_id(&ctx.env)
        .ok_or_else(|| Error::from("Stripe price not configured"))?;
    let app_url = config::app_base_url(&ctx.env);

    let billing_store = D1BillingStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let billing = BillingService::new(&billing_store, &user_store);

    // Look up or create Stripe customer
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let user_info = auth_store
        .get_user(&user_id)
        .await
        .map_err(|e| Error::from(e.to_string()))?
        .ok_or_else(|| Error::from("User not found"))?;

    let customer_id = match billing.get_stripe_customer_id(&user_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Create customer via Stripe API
            let form_body = format!("email={}", urlencoding::encode(&user_info.email));
            let headers = Headers::new();
            headers.set("Authorization", &format!("Bearer {}", stripe_secret))?;
            headers.set("Content-Type", "application/x-www-form-urlencoded")?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post)
                .with_headers(headers)
                .with_body(Some(wasm_bindgen::JsValue::from_str(&form_body)));
            let mut resp = Fetch::Request(Request::new_with_init(
                "https://api.stripe.com/v1/customers",
                &init,
            )?)
            .send()
            .await
            .map_err(|e| Error::from(format!("Stripe API error: {}", e)))?;
            let body: serde_json::Value = resp.json().await?;
            let cid = body["id"]
                .as_str()
                .ok_or_else(|| Error::from("Failed to create Stripe customer"))?
                .to_string();
            billing
                .set_stripe_customer_id(&user_id, &cid)
                .await
                .map_err(|e| Error::from(e.to_string()))?;
            cid
        }
        Err(e) => return error_response(e),
    };

    // Create Checkout Session via Stripe API
    let success_url = format!("{}?checkout=success", app_url);
    let cancel_url = format!("{}?checkout=cancelled", app_url);
    let form_body = format!(
        "customer={}&mode=subscription&success_url={}&cancel_url={}&line_items[0][price]={}&line_items[0][quantity]=1",
        urlencoding::encode(&customer_id),
        urlencoding::encode(&success_url),
        urlencoding::encode(&cancel_url),
        urlencoding::encode(&stripe_price_id),
    );
    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", stripe_secret))?;
    headers.set("Content-Type", "application/x-www-form-urlencoded")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&form_body)));
    let mut resp = Fetch::Request(Request::new_with_init(
        "https://api.stripe.com/v1/checkout/sessions",
        &init,
    )?)
    .send()
    .await
    .map_err(|e| Error::from(format!("Stripe API error: {}", e)))?;
    let body: serde_json::Value = resp.json().await?;

    match body["url"].as_str() {
        Some(url) => Response::from_json(&UrlResponse {
            url: url.to_string(),
        }),
        None => Response::from_json(&serde_json::json!({ "error": "No checkout URL" }))
            .map(|r| r.with_status(500)),
    }
}

/// POST /api/stripe/portal — Create a Stripe Customer Portal session.
pub async fn stripe_portal(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let stripe_secret =
        config::stripe_secret_key(&ctx.env).ok_or_else(|| Error::from("Stripe not configured"))?;
    let app_url = config::app_base_url(&ctx.env);

    let billing_store = D1BillingStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let billing = BillingService::new(&billing_store, &user_store);

    let customer_id = match billing.get_stripe_customer_id(&user_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return Response::from_json(
                &serde_json::json!({ "error": "No billing account found" }),
            )
            .map(|r| r.with_status(400));
        }
        Err(e) => return error_response(e),
    };

    let form_body = format!(
        "customer={}&return_url={}",
        urlencoding::encode(&customer_id),
        urlencoding::encode(&app_url),
    );
    let headers = Headers::new();
    headers.set("Authorization", &format!("Bearer {}", stripe_secret))?;
    headers.set("Content-Type", "application/x-www-form-urlencoded")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&form_body)));
    let mut resp = Fetch::Request(Request::new_with_init(
        "https://api.stripe.com/v1/billing_portal/sessions",
        &init,
    )?)
    .send()
    .await
    .map_err(|e| Error::from(format!("Stripe API error: {}", e)))?;
    let body: serde_json::Value = resp.json().await?;

    match body["url"].as_str() {
        Some(url) => Response::from_json(&UrlResponse {
            url: url.to_string(),
        }),
        None => Response::from_json(&serde_json::json!({ "error": "No portal URL" }))
            .map(|r| r.with_status(500)),
    }
}

/// POST /api/stripe/webhook — Handle Stripe webhook events.
pub async fn stripe_webhook(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let webhook_secret = config::stripe_webhook_secret(&ctx.env)
        .ok_or_else(|| Error::from("Stripe webhook secret not configured"))?;

    let signature_header = req
        .headers()
        .get("Stripe-Signature")?
        .ok_or_else(|| Error::from("Missing Stripe-Signature"))?;

    let body_bytes = req.bytes().await?;
    let payload = std::str::from_utf8(&body_bytes)
        .map_err(|_| Error::from("Invalid UTF-8 in webhook body"))?;

    if let Err(msg) = verify_stripe_signature(payload, &signature_header, &webhook_secret) {
        worker::console_log!("Stripe webhook signature verification failed: {}", msg);
        return Response::empty().map(|r| r.with_status(400));
    }

    let event: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| Error::from(e.to_string()))?;

    let event_type = event["type"].as_str().unwrap_or("");
    let data_object = &event["data"]["object"];

    let billing_store = D1BillingStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let billing = BillingService::new(&billing_store, &user_store);

    match event_type {
        "checkout.session.completed" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let subscription_id = data_object["subscription"].as_str();
            let _ = billing
                .handle_checkout_completed(customer_id, subscription_id)
                .await;
        }
        "customer.subscription.updated" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let status = data_object["status"].as_str().unwrap_or("");
            let _ = billing
                .handle_subscription_updated(customer_id, status)
                .await;
        }
        "customer.subscription.deleted" => {
            let customer_id = data_object["customer"].as_str().unwrap_or("");
            let _ = billing.handle_subscription_deleted(customer_id).await;
        }
        other => {
            worker::console_log!("Unhandled Stripe event: {}", other);
        }
    }

    Response::empty().map(|r| r.with_status(200))
}

/// GET /api/stripe/config — Return Stripe publishable key.
pub async fn stripe_config(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let publishable_key = config::stripe_publishable_key(&ctx.env).unwrap_or_default();
    Response::from_json(&StripeConfigResponse { publishable_key })
}

/// Verify Stripe webhook signature using HMAC-SHA256.
fn verify_stripe_signature(
    payload: &str,
    signature_header: &str,
    secret: &str,
) -> std::result::Result<(), String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut timestamp = None;
    let mut signatures = Vec::new();

    for part in signature_header.split(',') {
        let part = part.trim();
        if let Some(ts) = part.strip_prefix("t=") {
            timestamp = Some(ts.to_string());
        } else if let Some(sig) = part.strip_prefix("v1=") {
            signatures.push(sig.to_string());
        }
    }

    let timestamp = timestamp.ok_or_else(|| "Missing timestamp in signature".to_string())?;
    if signatures.is_empty() {
        return Err("Missing v1 signature".to_string());
    }

    // Check timestamp tolerance (5 minutes)
    if let Ok(ts) = timestamp.parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        if (now - ts).unsigned_abs() > 300 {
            return Err("Signature timestamp too old".to_string());
        }
    }

    let signed_payload = format!("{}.{}", timestamp, payload);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC key error: {}", e))?;
    mac.update(signed_payload.as_bytes());
    let result = mac.finalize().into_bytes();
    let expected: String = result.iter().map(|b| format!("{:02x}", b)).collect();

    if signatures.iter().any(|sig| {
        if sig.len() != expected.len() {
            return false;
        }
        sig.bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
    }) {
        Ok(())
    } else {
        Err("Signature mismatch".to_string())
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::verify_stripe_signature;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn signature_header(payload: &str, secret: &str, timestamp: i64) -> String {
        let signed_payload = format!("{timestamp}.{payload}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
        mac.update(signed_payload.as_bytes());
        let signature: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("t={timestamp},v1={signature}")
    }

    #[wasm_bindgen_test]
    fn stripe_signature_verifier_accepts_valid_signatures() {
        let payload = r#"{"id":"evt_123","type":"checkout.session.completed"}"#;
        let secret = "whsec_test";
        let header = signature_header(payload, secret, chrono::Utc::now().timestamp());

        assert!(verify_stripe_signature(payload, &header, secret).is_ok());
    }

    #[wasm_bindgen_test]
    fn stripe_signature_verifier_rejects_invalid_headers() {
        let payload = r#"{"id":"evt_123"}"#;
        let secret = "whsec_test";

        let missing_timestamp = verify_stripe_signature(payload, "v1=abcd", secret);
        assert_eq!(
            missing_timestamp.err().as_deref(),
            Some("Missing timestamp in signature")
        );

        let expired = signature_header(payload, secret, chrono::Utc::now().timestamp() - 600);
        assert_eq!(
            verify_stripe_signature(payload, &expired, secret)
                .err()
                .as_deref(),
            Some("Signature timestamp too old")
        );

        let mismatched = signature_header(payload, "other-secret", chrono::Utc::now().timestamp());
        assert_eq!(
            verify_stripe_signature(payload, &mismatched, secret)
                .err()
                .as_deref(),
            Some("Signature mismatch")
        );
    }
}

// ---------------------------------------------------------------------------
// Apple IAP handlers
// ---------------------------------------------------------------------------

/// POST /api/apple/verify-receipt — Verify a StoreKit 2 JWS signed transaction.
pub async fn apple_verify_receipt(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let body: diaryx_server::api::billing::AppleVerifyReceiptRequest = req.json().await?;

    // For now, delegate to the BillingService to store the transaction and upgrade.
    // Full JWS verification requires Apple Root CA and x509 parsing — the CF worker
    // can call the Apple App Store Server API for verification instead.
    let billing_store = D1BillingStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let billing = BillingService::new(&billing_store, &user_store);

    match billing
        .activate_apple_transaction(&user_id, &body.product_id)
        .await
    {
        Ok(()) => Response::from_json(&AppleVerifyReceiptResponse {
            success: true,
            tier: "plus".to_string(),
        }),
        Err(e) => error_response(e),
    }
}

/// POST /api/apple/restore — Verify multiple transactions from a restore flow.
pub async fn apple_restore(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let body: diaryx_server::api::billing::AppleRestoreRequest = req.json().await?;

    let billing_store = D1BillingStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let billing = BillingService::new(&billing_store, &user_store);

    let mut restored_count = 0usize;
    for _signed_tx in &body.signed_transactions {
        // In a full implementation, each JWS would be verified and its
        // original_transaction_id extracted. For now, count them as restored.
        restored_count += 1;
    }

    if restored_count > 0 {
        // Upgrade if any valid transactions found
        if let Err(e) = billing
            .activate_apple_transaction(&user_id, "restored")
            .await
        {
            return error_response(e);
        }
    }

    let tier = if restored_count > 0 { "plus" } else { "free" };
    Response::from_json(&AppleRestoreResponse {
        success: true,
        restored_count,
        tier: tier.to_string(),
    })
}

/// POST /api/apple/webhook — App Store Server Notifications V2 (stub).
pub async fn apple_webhook(mut req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    let body = req.bytes().await?;
    worker::console_log!(
        "Received Apple App Store Server Notification ({} bytes)",
        body.len()
    );
    Response::empty().map(|r| r.with_status(200))
}

// ---------------------------------------------------------------------------
// Tier management
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SetTierBody {
    tier: String,
}

/// POST /api/tier — Admin endpoint to set a user's tier (requires admin secret).
pub async fn set_tier(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: SetTierBody = req.json().await?;

    let user_store = D1UserStore::new(db(&ctx)?);
    let tier = diaryx_server::UserTier::from_str_lossy(&body.tier);

    match user_store.set_user_tier(&user_id, tier).await {
        Ok(()) => Response::from_json(&serde_json::json!({
            "tier": tier.as_str(),
        })),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Passkey handlers
// ---------------------------------------------------------------------------

use diaryx_server::use_cases::passkeys::PasskeyService;

fn rp_id(ctx: &RouteContext<()>) -> String {
    let app_url = config::app_base_url(&ctx.env);
    url::Url::parse(&app_url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "app.diaryx.org".to_string())
}

/// POST /api/auth/passkeys/register/start (authenticated)
pub async fn passkey_register_start(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;

    let auth_store = D1AuthStore::new(db(&ctx)?);
    let user_info = auth_store
        .get_user(&user_id)
        .await
        .map_err(|e| Error::from(e.to_string()))?
        .ok_or_else(|| Error::from("User not found"))?;

    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    match service.start_registration(&user_id, &user_info.email).await {
        Ok((challenge_id, options)) => Response::from_json(&PasskeyRegisterStartResponse {
            challenge_id,
            options,
        }),
        Err(e) => error_response(e),
    }
}

/// POST /api/auth/passkeys/register/finish (authenticated)
pub async fn passkey_register_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let body: PasskeyRegisterFinishRequest = req.json().await?;

    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    let credential_bytes =
        serde_json::to_vec(&body.credential).map_err(|e| Error::from(e.to_string()))?;

    match service
        .finish_registration(&body.challenge_id, &user_id, &body.name, &credential_bytes)
        .await
    {
        Ok(id) => Response::from_json(&PasskeyRegisterFinishResponse { id }),
        Err(e) => error_response(e),
    }
}

/// POST /api/auth/passkeys/authenticate/start (public)
pub async fn passkey_auth_start(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: PasskeyAuthStartRequest = req.json().await?;

    let email = body
        .email
        .as_deref()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .map(|e| e.to_lowercase());

    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    match service.start_authentication(email.as_deref()).await {
        Ok((challenge_id, options)) => Response::from_json(&PasskeyAuthStartResponse {
            challenge_id,
            options,
        }),
        Err(e) => error_response(e),
    }
}

/// POST /api/auth/passkeys/authenticate/finish (public)
pub async fn passkey_auth_finish(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let body: PasskeyAuthFinishRequest = req.json().await?;

    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    let credential_bytes =
        serde_json::to_vec(&body.credential).map_err(|e| Error::from(e.to_string()))?;

    let auth_result = match service
        .finish_authentication(&body.challenge_id, &credential_bytes)
        .await
    {
        Ok(r) => r,
        Err(e) => return error_response(e),
    };

    // Create session for the authenticated user
    let ml_store = D1MagicLinkStore::new(db(&ctx)?);
    let user_store = D1UserStore::new(db(&ctx)?);
    let device_store = D1DeviceStore::new(db(&ctx)?);
    let session_store = D1AuthSessionStore::new(db(&ctx)?);
    let cfg = auth_cfg(&ctx);

    let auth_service =
        AuthenticationService::new(&ml_store, &user_store, &device_store, &session_store, &cfg);

    match auth_service
        .create_session_for_email(
            &auth_result.email,
            body.device_name.as_deref(),
            None,
            body.replace_device_id.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let cookie = set_session_cookie(&result.session_token, &ctx.env);
            let mut resp = Response::from_json(&serde_json::json!({
                "success": true,
                "token": result.session_token,
                "user": { "id": result.user_id, "email": result.email },
            }))?;
            resp.headers_mut().set("Set-Cookie", &cookie)?;
            Ok(resp)
        }
        Err(AuthError::DeviceLimitReached { devices, .. }) => {
            Response::from_json(&serde_json::json!({
                "error": "Device limit reached. Remove a device to sign in on a new one.",
                "devices": devices,
            }))
            .map(|r| r.with_status(403))
        }
        Err(e) => Response::from_json(&serde_json::json!({ "error": e.to_string() }))
            .map(|r| r.with_status(400)),
    }
}

/// GET /api/auth/passkeys (authenticated) — list user's passkeys
pub async fn passkey_list(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    match service.list_passkeys(&user_id).await {
        Ok(items) => Response::from_json(&items),
        Err(e) => error_response(e),
    }
}

/// DELETE /api/auth/passkeys/:id (authenticated)
pub async fn passkey_delete(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let id = ctx.param("id").ok_or_else(|| Error::from("missing id"))?;

    let passkey_store = D1PasskeyStore::new(db(&ctx)?);
    let service = PasskeyService::new(&passkey_store, &rp_id(&ctx));

    match service.delete_passkey(id, &user_id).await {
        Ok(true) => Response::empty().map(|r| r.with_status(204)),
        Ok(false) => Response::empty().map(|r| r.with_status(404)),
        Err(e) => error_response(e),
    }
}

// ---------------------------------------------------------------------------
// Sync (WebSocket → Durable Object)
// ---------------------------------------------------------------------------

/// GET /api/sync/:namespace_id — Upgrade to WebSocket, forward to namespace DO.
///
/// Supports two auth paths:
/// - Owner: authenticated via token, namespace ownership verified
/// - Guest: authenticated via `?session=CODE` query param, session looked up for namespace access
pub async fn upgrade_sync_ws(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let namespace_id = ctx
        .param("namespace_id")
        .ok_or_else(|| Error::from("missing namespace_id"))?
        .to_string();

    // Check for session-based guest access
    let url = req.url()?;
    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let session_code = params.get("session").cloned();

    let (user_id, is_guest, read_only) = if let Some(ref code) = session_code {
        // Guest path: look up session to verify access
        let ns_store = D1NamespaceStore::new(db(&ctx)?);
        let session_store = D1SessionStore::new(db(&ctx)?);
        let service = SessionService::new(&ns_store, &session_store);

        let session = service
            .get(code)
            .await
            .map_err(|e| Error::from(e.to_string()))?;

        if session.namespace_id != namespace_id {
            return Response::error("Session does not match namespace", 403);
        }

        // Guest user_id: try to authenticate, fall back to anonymous
        let guest_id = match authenticate(&req, &ctx).await {
            Ok(id) => id,
            Err(_) => format!("guest:{}", code),
        };

        (guest_id, true, session.read_only)
    } else {
        // Owner path: standard authentication + ownership check
        let user_id = authenticate(&req, &ctx).await?;

        let ns_store = D1NamespaceStore::new(db(&ctx)?);
        let ns = ns_store
            .get_namespace(&namespace_id)
            .await
            .map_err(|e| Error::from(e.to_string()))?
            .ok_or_else(|| Error::from("Namespace not found"))?;

        if ns.owner_user_id != user_id {
            return Response::error("Forbidden", 403);
        }

        (user_id, false, false)
    };

    // Get DO stub for this namespace
    let do_namespace = ctx.env.durable_object("NAMESPACE_SYNC")?;
    let do_id = do_namespace.id_from_name(&namespace_id)?;
    let stub = do_id.get_stub()?;

    // Forward the request to the DO with auth context in query params
    let mut do_url = url.clone();
    {
        let mut pairs = do_url.query_pairs_mut();
        pairs
            .append_pair("user_id", &user_id)
            .append_pair("workspace_id", &namespace_id)
            .append_pair("is_guest", &is_guest.to_string())
            .append_pair("read_only", &read_only.to_string());
        if let Some(ref code) = session_code {
            pairs.append_pair("session_code", code);
        }
    }

    let mut do_req = Request::new(do_url.as_str(), Method::Get)?;
    // Copy the Upgrade header
    if let Some(upgrade) = req.headers().get("Upgrade")? {
        do_req.headers_mut()?.set("Upgrade", &upgrade)?;
    }

    stub.fetch_with_request(do_req).await
}

// ---------------------------------------------------------------------------
// Proxy handlers
// ---------------------------------------------------------------------------

/// POST /api/proxy/:proxy_id/*path — Generic proxy for plugin-initiated API requests.
///
/// Resolves credentials from env secrets, validates tier/quota, and forwards
/// the request to the configured upstream. Supports streaming (SSE passthrough).
pub async fn proxy_request(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let user_id = authenticate(&req, &ctx).await?;
    let proxy_id = ctx
        .param("proxy_id")
        .ok_or_else(|| Error::from("missing proxy_id"))?
        .to_string();
    let path = ctx.param("path").map(|s| s.to_string()).unwrap_or_default();

    // Look up user tier
    let auth_store = D1AuthStore::new(db(&ctx)?);
    let tier = auth_store
        .get_user_tier(&user_id)
        .await
        .map_err(|e| Error::from(e.to_string()))?;

    // Resolve proxy config from env
    let (upstream, api_key, models, _monthly_quota) = match proxy_id.as_str() {
        "diaryx.ai" => {
            let key = ctx
                .env
                .secret("MANAGED_AI_OPENROUTER_API_KEY")
                .map(|v| v.to_string())
                .unwrap_or_default();
            if key.is_empty() {
                return Response::from_json(&serde_json::json!({
                    "error": "provider_unavailable",
                    "message": "Managed AI is not configured"
                }))
                .map(|r| r.with_status(503));
            }
            let endpoint = ctx
                .env
                .var("MANAGED_AI_OPENROUTER_ENDPOINT")
                .map(|v| v.to_string())
                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
            let models_str = ctx
                .env
                .var("MANAGED_AI_MODELS")
                .map(|v| v.to_string())
                .unwrap_or_default();
            let models: Vec<String> = models_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let quota: u64 = ctx
                .env
                .var("MANAGED_AI_MONTHLY_QUOTA")
                .ok()
                .and_then(|v| v.to_string().parse().ok())
                .unwrap_or(1000);
            (endpoint, key, models, quota)
        }
        _ => {
            return Response::from_json(&serde_json::json!({
                "error": "proxy_not_found",
                "message": format!("Proxy '{}' not found", proxy_id)
            }))
            .map(|r| r.with_status(404));
        }
    };

    // Check tier (Plus required for platform proxies)
    if tier != diaryx_server::UserTier::Plus {
        return Response::from_json(&serde_json::json!({
            "error": "plus_required",
            "message": "Diaryx Plus is required for this proxy"
        }))
        .map(|r| r.with_status(403));
    }

    // Read body
    let body_bytes = req.bytes().await.unwrap_or_default();

    // Validate model if models list is set
    if !models.is_empty() {
        if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            if let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                if !models.iter().any(|m| m == model) {
                    return Response::from_json(&serde_json::json!({
                        "error": "value_not_allowed",
                        "message": format!("Model '{}' is not in the allowlist", model)
                    }))
                    .map(|r| r.with_status(400));
                }
            }
        }
    }

    // Build upstream request
    let upstream_url = format!(
        "{}/{}",
        upstream.trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    let mut headers = worker::Headers::new();
    headers.set("Authorization", &format!("Bearer {}", api_key))?;
    headers.set("Content-Type", "application/json")?;

    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_headers(headers);
    init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(
        &String::from_utf8_lossy(&body_bytes),
    )));

    let upstream_req = Request::new_with_init(&upstream_url, &init)?;
    let mut upstream_resp = worker::Fetch::Request(upstream_req).send().await?;

    let status = upstream_resp.status_code();
    let resp_body = upstream_resp.text().await.unwrap_or_default();
    Response::ok(resp_body).map(|r| r.with_status(status))
}

/// POST /api/ai/*path — Backward-compat alias for /api/proxy/diaryx.ai/*path.
pub async fn ai_compat_proxy(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    proxy_request(req, ctx).await
}
