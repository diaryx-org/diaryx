//! Native-target test helpers for `diaryx_sync_server`.
//!
//! Provides a real-TCP test server ([`TestServer`]) that spawns the full
//! router on `127.0.0.1:0` with `:memory:` SQLite + [`InMemoryBlobStore`],
//! plus a dev-mode sign-in helper. Used by
//! `crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs` to drive two
//! plugin instances against a real HTTP server.
//!
//! Distinct from [`tests/support/mod.rs`](../../tests/support/mod.rs), which
//! uses `tower::ServiceExt::oneshot` for in-process HTTP. That harness is
//! faster but can't satisfy plugin code that makes real socket calls via
//! `ureq`.
//!
//! # Example
//!
//! ```ignore
//! let server = TestServer::start().await;
//! let token = server.sign_in_dev("alice@example.com").await;
//! let provider = HttpNamespaceProvider::new(server.base_url(), Some(token));
//! // ... drive plugin harness against `provider` ...
//! drop(server); // clean shutdown
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use rusqlite::Connection;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::adapters::{
    NativeAuthSessionStore, NativeAuthStore, NativeNamespaceStore, NativeObjectMetaStore,
    NativeSessionStore, NativeUserStore,
};
use crate::auth::{MagicLinkService, PasskeyService};
use crate::blob_store::{BlobStore, InMemoryBlobStore};
use crate::config::{AppleIapConfig, Config, EmailConfig, ManagedAiConfig, R2Config, StripeConfig};
use crate::db::{AuthRepo, NamespaceRepo, init_database};
use crate::email::EmailService;
use crate::handlers::{
    AudienceState, NamespaceState, NsSessionState, ObjectState, audience_routes, auth_routes,
    namespace_routes, ns_session_routes, object_routes, public_object_routes, usage_routes,
};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Build a [`Config`] with safe defaults for tests: no email, no billing,
/// plain-HTTP cookies, in-memory blob store. Callers can mutate the returned
/// value before wrapping it in `Arc`.
pub fn test_config() -> Config {
    Config {
        host: "127.0.0.1".to_string(),
        port: 0,
        database_path: PathBuf::from(":memory:"),
        app_base_url: "http://localhost:5174".to_string(),
        email: EmailConfig {
            api_key: String::new(), // empty → dev-mode magic links in response
            from_email: "test@example.invalid".to_string(),
            from_name: "Diaryx Test".to_string(),
        },
        session_expiry_days: 30,
        magic_link_expiry_minutes: 15,
        cors_origins: vec!["http://localhost:5174".to_string()],
        r2: R2Config {
            bucket: "test".to_string(),
            account_id: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            endpoint: None,
            prefix: "test".to_string(),
        },
        token_signing_key: vec![0u8; 32], // 32-byte zero key — test-only
        admin_secret: None,
        managed_ai: ManagedAiConfig {
            openrouter_api_key: String::new(),
            openrouter_endpoint: "https://example.invalid".to_string(),
            models: Vec::new(),
            rate_limit_per_minute: 0,
            monthly_quota: 0,
        },
        stripe: Option::<StripeConfig>::None,
        apple_iap: Option::<AppleIapConfig>::None,
        secure_cookies: false,
        blob_store_path: PathBuf::from("./test-blobs"),
        blob_store_in_memory: true,
        kv_api_token: None,
        kv_namespace_id: None,
        site_base_url: "http://localhost:5174".to_string(),
        site_domain: None,
    }
}

// ---------------------------------------------------------------------------
// Router assembly
// ---------------------------------------------------------------------------

/// Build the subset of the full router needed for plugin E2E scenarios:
/// health + auth + namespace + object + audience + usage + sessions + public
/// object access. Omits: sync-v2 websockets, AI proxy, Stripe, Apple IAP,
/// domain management. Add them back by extending this function when a test
/// needs them.
fn build_e2e_router(
    config: Arc<Config>,
    repo: Arc<AuthRepo>,
) -> (Router, axum::Extension<crate::auth::AuthExtractor>) {
    let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));
    let magic_link_service = Arc::new(MagicLinkService::new(repo.clone(), config.clone()));
    let email_service = Arc::new(EmailService::new(config.clone()));
    let passkey_service = Arc::new(PasskeyService::new(
        repo.clone(),
        config.clone(),
        magic_link_service.clone(),
    ));

    let auth_session_store = Arc::new(NativeAuthSessionStore::new(repo.clone()));
    let user_store = Arc::new(NativeUserStore::new(repo.clone()));
    let auth_store = Arc::new(NativeAuthStore::new(repo.clone()));
    let namespace_store = Arc::new(NativeNamespaceStore::new(ns_repo.clone()));
    let session_store = Arc::new(NativeSessionStore::new(ns_repo.clone()));
    let object_meta_store = Arc::new(NativeObjectMetaStore::new(ns_repo));
    let blob_store: Arc<dyn BlobStore> = Arc::new(InMemoryBlobStore::new("test"));

    let auth_extractor =
        crate::auth::AuthExtractor::new(auth_store.clone(), auth_session_store.clone());

    let auth_state = crate::handlers::auth::AuthState {
        magic_link_service,
        email_service,
        auth_store,
        namespace_store: namespace_store.clone(),
        session_store: auth_session_store,
        user_store,
        passkey_service,
        session_expiry_days: config.session_expiry_days,
        secure_cookies: config.secure_cookies,
    };
    let namespace_state = NamespaceState {
        namespace_store: namespace_store.clone(),
        domain_mapping_cache: None, // domains not wired in test router
    };
    let object_state = ObjectState {
        namespace_store: namespace_store.clone(),
        object_meta_store,
        blob_store: blob_store.clone(),
        token_signing_key: config.token_signing_key.clone(),
    };
    let audience_state = AudienceState {
        namespace_store: namespace_store.clone(),
        token_signing_key: config.token_signing_key.clone(),
        blob_store,
    };
    let ns_session_state = NsSessionState {
        namespace_store,
        session_store,
    };

    let api = Router::new()
        .route("/health", get(|| async { "OK" }))
        .nest("/auth", auth_routes(auth_state))
        .nest("/namespaces", namespace_routes(namespace_state))
        .nest("/namespaces/{ns_id}", object_routes(object_state.clone()))
        .nest("/namespaces/{ns_id}", audience_routes(audience_state))
        .merge(public_object_routes(object_state.clone()))
        .nest("/usage", usage_routes(object_state))
        .nest("/sessions", ns_session_routes(ns_session_state));

    let router = Router::new().nest("/api", api);
    (router, axum::Extension(auth_extractor))
}

// ---------------------------------------------------------------------------
// TestServer
// ---------------------------------------------------------------------------

/// A running instance of `diaryx_sync_server` bound to `127.0.0.1` on an
/// OS-assigned port. Dropping it signals the background task to shut down.
///
/// # URL conventions
///
/// - [`base_url`](Self::base_url) → `http://127.0.0.1:PORT` (origin only).
///   Consumers that speak the Diaryx HTTP contract (e.g. `ReqwestDispatcher`)
///   include the `/api` prefix in their paths; this keeps the convention
///   consistent with how Axum / reqwest / Cloudflare all model base URLs.
/// - [`api_base_url`](Self::api_base_url) → `http://127.0.0.1:PORT/api`.
///   A convenience for consumers that expect the API prefix baked in
///   (most notably `HttpNamespaceProvider`, which concatenates
///   `/namespaces/...` after the base).
pub struct TestServer {
    base_url: String,
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    config: Arc<Config>,
    shutdown: CancellationToken,
    task: Option<JoinHandle<()>>,
    client: reqwest::Client,
}

impl TestServer {
    /// Start a server in the current tokio runtime on `127.0.0.1:0`. Returns
    /// once the listener is bound (so `base_url()` is immediately valid).
    pub async fn start() -> Self {
        let config = Arc::new(test_config());

        let conn = Connection::open_in_memory().expect("open :memory: sqlite");
        init_database(&conn).expect("init schema");
        let repo = Arc::new(AuthRepo::new(conn));

        let (router, auth_ext) = build_e2e_router(config.clone(), repo);
        let app = router.layer(auth_ext);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind 127.0.0.1:0");
        let addr = listener.local_addr().expect("listener addr");
        let port = addr.port();
        let base_url = format!("http://127.0.0.1:{port}");

        let shutdown = CancellationToken::new();
        let shutdown_child = shutdown.clone();
        let task = tokio::spawn(async move {
            let serve =
                axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
                    shutdown_child.cancelled().await;
                });
            if let Err(e) = serve.await {
                eprintln!("TestServer: axum::serve returned error: {e}");
            }
        });

        Self {
            base_url,
            port,
            config,
            shutdown,
            task: Some(task),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    /// Origin URL — `http://127.0.0.1:PORT`, no path prefix. Intended for
    /// HTTP clients that include `/api/...` in their paths (e.g.
    /// `ReqwestDispatcher` for the contract suite).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `http://127.0.0.1:PORT/api` — for consumers that expect the API
    /// prefix baked into the base, most notably `HttpNamespaceProvider`.
    pub fn api_base_url(&self) -> String {
        format!("{}/api", self.base_url)
    }

    /// Complete the dev-mode magic-link → verify dance and return a session
    /// token. Requires the server to be running with dev-mode email (which
    /// is the default for [`test_config`]).
    pub async fn sign_in_dev(&self, email: &str) -> String {
        // POST /api/auth/magic-link
        let ml_url = format!("{}/api/auth/magic-link", self.base_url);
        let ml_body = serde_json::json!({ "email": email });
        let ml_resp: serde_json::Value = self
            .client
            .post(&ml_url)
            .json(&ml_body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("magic-link POST failed: {e}"))
            .error_for_status()
            .unwrap_or_else(|e| panic!("magic-link non-2xx: {e}"))
            .json()
            .await
            .unwrap_or_else(|e| panic!("magic-link response parse: {e}"));

        let dev_link = ml_resp
            .get("dev_link")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("magic-link dev_mode response missing dev_link: {ml_resp}"));

        // Extract ?token=... from dev_link. Format: {app_base_url}?token={token}
        let token = dev_link
            .split("token=")
            .nth(1)
            .unwrap_or_else(|| panic!("dev_link missing token= param: {dev_link}"))
            .split('&')
            .next()
            .unwrap_or("")
            .to_string();

        // GET /api/auth/verify?token=<TOKEN>&device_name=e2e
        let verify_url = format!(
            "{}/api/auth/verify?token={}&device_name=e2e",
            self.base_url, token
        );
        let verify_resp: serde_json::Value = self
            .client
            .get(&verify_url)
            .send()
            .await
            .unwrap_or_else(|e| panic!("verify GET failed: {e}"))
            .error_for_status()
            .unwrap_or_else(|e| panic!("verify non-2xx: {e}"))
            .json()
            .await
            .unwrap_or_else(|e| panic!("verify response parse: {e}"));

        verify_resp
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("verify response missing token: {verify_resp}"))
            .to_string()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
