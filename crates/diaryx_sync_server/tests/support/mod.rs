//! Shared HTTP test harness for `diaryx_sync_server` integration tests.
//!
//! Builds an in-process [`Router`] wired to `:memory:` SQLite plus the
//! in-memory blob store, with email and billing disabled. Tests drive it via
//! [`tower::ServiceExt::oneshot`] — no network, no ports, no fixture server.
//!
//! Status: seed. Currently exposes only the auth routes plus `/api/health`.
//! Extending to object/audience/namespace routes is a matter of wiring the
//! existing handler states into [`build_test_router`].
//!
//! # Note on dead-code warnings
//!
//! Each `tests/*.rs` integration test is its own binary and gets a fresh
//! copy of this module. Helpers used only by a subset of test files will
//! look unused from the perspective of the others, hence the blanket
//! `#![allow(dead_code)]`.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode, header};
use axum::{Router, routing::get};
use rusqlite::Connection;
use tower::ServiceExt;

use diaryx_sync_server::adapters::{
    NativeAuthSessionStore, NativeAuthStore, NativeNamespaceStore, NativeUserStore,
};
use diaryx_sync_server::auth::{MagicLinkService, PasskeyService};
use diaryx_sync_server::config::{
    AppleIapConfig, Config, EmailConfig, ManagedAiConfig, R2Config, StripeConfig,
};
use diaryx_sync_server::db::{AuthRepo, NamespaceRepo, init_database};
use diaryx_sync_server::email::EmailService;
use diaryx_sync_server::handlers::auth::{AuthState, auth_routes};

// ---------------------------------------------------------------------------
// Config construction
// ---------------------------------------------------------------------------

/// Build a [`Config`] with safe defaults for tests: no email, no billing, no
/// R2, plain-HTTP cookies. Callers can mutate the returned value before
/// wrapping it in `Arc` if they need to exercise a specific config path.
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
        // 32-byte zero key — fine for tests; don't ship this to prod.
        token_signing_key: vec![0u8; 32],
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
// Router construction
// ---------------------------------------------------------------------------

/// A fully-built test app, plus handles to shared state so tests can make
/// assertions directly against the database or issue follow-up requests.
///
/// `repo` and `config` aren't consumed by the seed tests but are exposed
/// deliberately — subsequent handler tests will reach into them to seed
/// fixtures or inspect state.
#[allow(dead_code)]
pub struct TestApp {
    pub router: Router,
    pub repo: Arc<AuthRepo>,
    pub config: Arc<Config>,
}

impl TestApp {
    /// Issue a request against the router. Consumes and clones the router so
    /// a single `TestApp` can serve many requests.
    pub async fn request(&self, req: Request<Body>) -> Response<Body> {
        self.router
            .clone()
            .oneshot(req)
            .await
            .expect("router must not fail")
    }

    pub async fn get(&self, path: &str) -> Response<Body> {
        let req = Request::builder()
            .method(Method::GET)
            .uri(path)
            .body(Body::empty())
            .expect("request builder");
        self.request(req).await
    }

    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Response<Body> {
        let req = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("json body")))
            .expect("request builder");
        self.request(req).await
    }
}

/// Build a minimal test router mounting only `/api/health` and `/api/auth/*`.
/// Extend as additional handler states are needed.
pub fn build_test_router() -> TestApp {
    let config = Arc::new(test_config());

    let conn = Connection::open_in_memory().expect("open :memory: sqlite");
    init_database(&conn).expect("init schema");
    let repo = Arc::new(AuthRepo::new(conn));

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
    let namespace_store = Arc::new(NativeNamespaceStore::new(ns_repo));

    let auth_state = AuthState {
        magic_link_service,
        email_service,
        auth_store,
        namespace_store,
        session_store: auth_session_store,
        user_store,
        passkey_service,
        session_expiry_days: config.session_expiry_days,
        secure_cookies: config.secure_cookies,
    };

    let api = Router::new()
        .route("/health", get(|| async { "OK" }))
        .nest("/auth", auth_routes(auth_state));

    let router = Router::new().nest("/api", api);

    TestApp {
        router,
        repo,
        config,
    }
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

pub async fn read_body(response: Response<Body>) -> Vec<u8> {
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body")
        .to_vec()
}

pub async fn read_json(response: Response<Body>) -> serde_json::Value {
    let bytes = read_body(response).await;
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "expected JSON response body but got: {e}; body: {}",
            String::from_utf8_lossy(&bytes)
        )
    })
}

pub async fn read_status_and_json(response: Response<Body>) -> (StatusCode, serde_json::Value) {
    let status = response.status();
    (status, read_json(response).await)
}
