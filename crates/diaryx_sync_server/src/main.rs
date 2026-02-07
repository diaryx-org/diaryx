use axum::{
    Router,
    extract::Extension,
    http::{Method, header},
    routing::get,
};
use diaryx_sync_server::{
    auth::{AuthExtractor, MagicLinkService},
    config::Config,
    db::{AuthRepo, init_database},
    email::EmailService,
    handlers::{api_routes, auth_routes, session_routes},
    sync_v2::SyncV2Server,
};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info};
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
    let auth_extractor = AuthExtractor::new(repo.clone());

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

    // Create sync v2 server (siphonophore-based)
    let sync_v2_server = SyncV2Server::new(repo.clone(), workspaces_dir.clone());
    let sync_v2_state = Arc::new(sync_v2_server.state());
    let sync_v2_router = sync_v2_server.into_router_at("/sync2");

    // Create handler states
    let auth_state = diaryx_sync_server::handlers::auth::AuthState {
        magic_link_service,
        email_service,
        repo: repo.clone(),
        workspaces_dir: Some(workspaces_dir.clone()),
    };

    let api_state = diaryx_sync_server::handlers::api::ApiState {
        repo: repo.clone(),
        sync_v2: sync_v2_state.clone(),
    };

    let sessions_state = diaryx_sync_server::handlers::sessions::SessionsState {
        repo: repo.clone(),
        sync_v2: sync_v2_state.clone(),
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
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_credentials(true)
        .allow_origin(AllowOrigin::list(origins));

    // Build the router
    let app = Router::new()
        // Health check
        .route("/", get(|| async { "Diaryx Sync Server" }))
        .route("/health", get(|| async { "OK" }))
        // Auth routes
        .nest("/auth", auth_routes(auth_state))
        // API routes
        .nest("/api", api_routes(api_state))
        // Session routes (for live share)
        .nest("/api/sessions", session_routes(sessions_state))
        // Sync v2 endpoint (siphonophore-based)
        .merge(sync_v2_router)
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
            let _ = cleanup_repo.cleanup_expired_share_sessions();
            info!("Cleaned up expired tokens, sessions, and share sessions");
        }
    });

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
