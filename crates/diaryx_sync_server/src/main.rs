use axum::{
    Router,
    extract::Extension,
    http::{Method, header},
    routing::get,
};
use diaryx_sync_server::{
    adapters::{
        NativeAuthSessionStore, NativeAuthStore, NativeDomainMappingCache, NativeNamespaceStore,
        NativeObjectMetaStore, NativeSessionStore, NativeUserStore,
    },
    auth::{AuthExtractor, MagicLinkService, PasskeyService},
    blob_store::{BlobStore, build_blob_store},
    config::Config,
    db::NamespaceRepo,
    db::{AuthRepo, init_database},
    email::EmailService,
    handlers::{
        AudienceState, DomainState, NamespaceState, NsSessionState, ObjectState, ai_routes,
        audience_routes, auth_routes, domain_auth_route, domain_routes, namespace_routes,
        ns_session_routes, object_routes, public_object_routes, usage_routes,
    },
    sync_v2::SyncV2Server,
};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "diaryx_sync_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = match Config::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    info!("Starting Diaryx Sync Server v{}", env!("CARGO_PKG_VERSION"));
    info!("Database path: {:?}", config.database_path);
    info!("CORS origins: {:?}", config.cors_origins);

    // Initialize database
    let conn = match Connection::open(&config.database_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to open database: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = init_database(&conn) {
        error!("Failed to initialize database: {}", e);
        std::process::exit(1);
    }

    // Create shared state
    let repo = Arc::new(AuthRepo::new(conn));
    let magic_link_service = Arc::new(MagicLinkService::new(repo.clone(), config.clone()));
    let email_service = Arc::new(EmailService::new(config.clone()));
    let passkey_service = Arc::new(PasskeyService::new(
        repo.clone(),
        config.clone(),
        magic_link_service.clone(),
    ));
    let auth_session_store = Arc::new(NativeAuthSessionStore::new(repo.clone()));
    let user_store = Arc::new(NativeUserStore::new(repo.clone()));
    let blob_store: Arc<dyn BlobStore> = match build_blob_store(config.as_ref()).await {
        Ok(store) => {
            if config.is_r2_configured() {
                info!("Blob store: R2 ({})", config.r2.bucket);
            } else if config.blob_store_in_memory {
                info!("Blob store: in-memory (volatile)");
            } else {
                info!(
                    "Blob store: local filesystem ({:?})",
                    config.blob_store_path
                );
            }
            store
        }
        Err(err) => {
            error!("Failed to initialize blob store: {}", err);
            std::process::exit(1);
        }
    };

    // Create data directory for workspace databases
    let data_dir = config
        .database_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let workspaces_dir = data_dir.join("workspaces");
    if let Err(e) = std::fs::create_dir_all(&workspaces_dir) {
        error!("Failed to create workspaces directory: {}", e);
        std::process::exit(1);
    }

    // Create namespace repo (shared connection from AuthRepo)
    let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));
    let auth_store = Arc::new(NativeAuthStore::new(repo.clone()));
    let namespace_store = Arc::new(NativeNamespaceStore::new(ns_repo.clone()));
    let domain_mapping_cache = Arc::new(NativeDomainMappingCache::new(
        reqwest::Client::new(),
        config.r2.account_id.clone(),
        config.kv_api_token.clone(),
        config.kv_namespace_id.clone(),
    ));

    // Create sync server (GenericNamespaceSyncHook)
    let sync_server = SyncV2Server::new(repo.clone(), ns_repo.clone(), workspaces_dir);
    let sync_router = sync_server.into_router_at("/namespaces/{ns_id}/sync");

    // Create shared rate limiter
    let rate_limiter = diaryx_sync_server::rate_limit::RateLimiter::new();

    let auth_extractor = AuthExtractor::new(auth_store.clone(), auth_session_store.clone());

    // Create handler states
    let auth_state = diaryx_sync_server::handlers::auth::AuthState {
        magic_link_service,
        email_service,
        auth_store,
        namespace_store: namespace_store.clone(),
        session_store: auth_session_store,
        user_store: user_store.clone(),
        passkey_service,
        session_expiry_days: config.session_expiry_days,
        secure_cookies: config.secure_cookies,
    };

    let ai_state = diaryx_sync_server::handlers::ai::AiState {
        repo: repo.clone(),
        rate_limiter: Arc::new(rate_limiter.clone()),
        http_client: reqwest::Client::new(),
        openrouter_api_key: config.managed_ai.openrouter_api_key.clone(),
        openrouter_endpoint: config.managed_ai.openrouter_endpoint.clone(),
        allowed_models: config.managed_ai.models.iter().cloned().collect(),
        rate_limit_per_minute: config.managed_ai.rate_limit_per_minute,
        monthly_quota: config.managed_ai.monthly_quota,
    };

    if config.managed_ai.openrouter_api_key.is_empty() {
        info!("Managed AI proxy: disabled (MANAGED_AI_OPENROUTER_API_KEY not set)");
    } else {
        info!(
            "Managed AI proxy: enabled (models={}, rate_limit={}/min, monthly_quota={})",
            config.managed_ai.models.join(","),
            config.managed_ai.rate_limit_per_minute,
            config.managed_ai.monthly_quota
        );
    }

    let session_store = Arc::new(NativeSessionStore::new(ns_repo.clone()));
    let object_meta_store = Arc::new(NativeObjectMetaStore::new(ns_repo.clone()));

    // Namespace / object / audience states
    let namespace_state = NamespaceState {
        namespace_store: namespace_store.clone(),
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
        blob_store: blob_store.clone(),
    };
    let domain_state = DomainState {
        ns_repo: ns_repo.clone(),
        namespace_store,
        domain_mapping_cache,
        blob_store: blob_store.clone(),
        token_signing_key: config.token_signing_key.clone(),
    };
    let ns_session_state = NsSessionState {
        namespace_store: namespace_state.namespace_store.clone(),
        session_store,
    };

    // Create Stripe state (if configured)
    let stripe_router = if let Some(stripe_config) = config.stripe.clone() {
        info!("Stripe billing: enabled (price={})", stripe_config.price_id);
        let stripe_state = diaryx_sync_server::handlers::stripe::StripeState {
            repo: repo.clone(),
            user_store: user_store.clone(),
            config: stripe_config,
            app_base_url: config.app_base_url.clone(),
        };
        Some(diaryx_sync_server::handlers::stripe::stripe_routes(
            stripe_state,
        ))
    } else {
        info!("Stripe billing: disabled (STRIPE_SECRET_KEY not set)");
        None
    };

    // Create Apple IAP state (if configured)
    let apple_iap_router = if let Some(apple_config) = config.apple_iap.clone() {
        info!(
            "Apple IAP: enabled (bundle_id={}, env={})",
            apple_config.bundle_id, apple_config.environment
        );
        if apple_config.skip_signature_verify {
            warn!(
                "⚠️  APPLE_IAP_SKIP_SIGNATURE_VERIFY is enabled — JWS signature verification is DISABLED. Do NOT use in production!"
            );
        }
        let apple_state = diaryx_sync_server::handlers::apple::AppleIapState {
            repo: repo.clone(),
            user_store: user_store.clone(),
            config: apple_config,
        };
        Some(diaryx_sync_server::handlers::apple::apple_iap_routes(
            apple_state,
        ))
    } else {
        info!("Apple IAP: disabled (APPLE_IAP_BUNDLE_ID not set)");
        None
    };

    // Build CORS layer
    let origins: Vec<_> = config
        .cors_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::CACHE_CONTROL,
            header::PRAGMA,
            header::COOKIE,
            header::HeaderName::from_static("x-audience"),
        ])
        .allow_credentials(true)
        .allow_origin(AllowOrigin::list(origins));

    // Build the router
    let mut app = Router::new()
        // Health check
        .route("/", get(|| async { "Diaryx Sync Server" }))
        .route("/health", get(|| async { "OK" }))
        // Auth routes
        .nest("/auth", auth_routes(auth_state))
        // AI routes
        .nest("/api", ai_routes(ai_state))
        // Generic namespace routes
        .nest("/namespaces", namespace_routes(namespace_state))
        // Object store routes (mounted under /namespaces/{ns_id})
        .nest("/namespaces/{ns_id}", object_routes(object_state.clone()))
        // Audience routes (mounted under /namespaces/{ns_id})
        .nest("/namespaces/{ns_id}", audience_routes(audience_state))
        // Domain management routes (mounted under /namespaces/{ns_id})
        .nest("/namespaces/{ns_id}", domain_routes(domain_state.clone()))
        // Public (unauthenticated) object access
        .merge(public_object_routes(object_state.clone()))
        // Caddy forward_auth endpoint
        .merge(domain_auth_route(domain_state))
        // Usage metering route (user-level, not namespace-scoped)
        .nest("/usage", usage_routes(object_state))
        // Namespace session routes
        .nest("/sessions", ns_session_routes(ns_session_state))
        // Generic namespace sync endpoint
        .merge(sync_router);

    // Stripe billing routes (only if configured)
    if let Some(stripe) = stripe_router {
        app = app.nest("/api", stripe);
    }

    // Apple IAP routes (only if configured)
    if let Some(apple) = apple_iap_router {
        app = app.nest("/api", apple);
    }

    let app = app
        // Add layers
        .layer(Extension(auth_extractor))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    // Create listener
    let addr = config.server_addr();
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    info!("Server listening on http://{}", addr);

    // Start cleanup task
    let cleanup_repo = repo.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let _ = cleanup_repo.cleanup_expired_magic_tokens();
            let _ = cleanup_repo.cleanup_expired_sessions();
            let _ = cleanup_repo.cleanup_expired_passkey_challenges();
            info!("Cleaned up expired tokens, sessions, and passkey challenges");
        }
    });

    // Start rate limiter cleanup task
    {
        let rl = rate_limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                rl.cleanup(std::time::Duration::from_secs(3600));
            }
        });
    }

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    info!("Server shut down gracefully");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}
