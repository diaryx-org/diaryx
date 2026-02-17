use axum::{
    Router,
    extract::Extension,
    http::{Method, header},
    routing::get,
};
use diaryx_sync_server::{
    auth::{AuthExtractor, MagicLinkService},
    blob_store::{BlobStore, build_blob_store, build_sites_store},
    config::Config,
    db::{AuthRepo, init_database},
    email::EmailService,
    handlers::sites::verify_domain_route,
    handlers::{api_routes, auth_routes, session_routes, site_routes},
    kv_client::CloudflareKvClient,
    publish::{
        new_publish_lock, publish_workspace_to_r2, release_publish_lock, try_acquire_publish_lock,
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
    let auth_extractor = AuthExtractor::new(repo.clone());
    let blob_store: Arc<dyn BlobStore> = match build_blob_store(config.as_ref()).await {
        Ok(store) => {
            if config.is_r2_configured() {
                info!("Attachment blob store: R2 ({})", config.r2.bucket);
            } else {
                info!("Attachment blob store: in-memory (R2 not configured)");
            }
            store
        }
        Err(err) => {
            error!("Failed to initialize blob store: {}", err);
            std::process::exit(1);
        }
    };
    let sites_store: Arc<dyn BlobStore> = match build_sites_store(config.as_ref()).await {
        Ok(store) => {
            if config.is_r2_configured() {
                info!("Sites blob store: R2 ({})", config.sites_r2_bucket);
            } else {
                info!("Sites blob store: in-memory (R2 not configured)");
            }
            store
        }
        Err(err) => {
            error!("Failed to initialize sites blob store: {}", err);
            std::process::exit(1);
        }
    };
    let publish_lock = new_publish_lock();

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
        blob_store: blob_store.clone(),
    };

    let api_state = diaryx_sync_server::handlers::api::ApiState {
        repo: repo.clone(),
        sync_v2: sync_v2_state.clone(),
        blob_store: blob_store.clone(),
        snapshot_upload_max_bytes: config.snapshot_upload_max_bytes,
        attachment_incremental_sync_enabled: config.attachment_incremental_sync_enabled,
        admin_secret: config.admin_secret.clone(),
    };

    let sessions_state = diaryx_sync_server::handlers::sessions::SessionsState {
        repo: repo.clone(),
        sync_v2: sync_v2_state.clone(),
    };
    let kv_client = if config.is_kv_configured() {
        Some(Arc::new(CloudflareKvClient::new(
            config.r2.account_id.clone(),
            config.kv_namespace_id.clone(),
            config.kv_api_token.clone(),
        )))
    } else {
        None
    };

    let sites_state = diaryx_sync_server::handlers::sites::SitesState {
        repo: repo.clone(),
        sync_v2: sync_v2_state.clone(),
        sites_store: sites_store.clone(),
        attachments_store: blob_store.clone(),
        token_signing_key: config.token_signing_key.clone(),
        sites_base_url: config.sites_base_url.clone(),
        publish_lock: publish_lock.clone(),
        kv_client,
    };

    // Create Stripe state (if configured)
    let stripe_router = if let Some(stripe_config) = config.stripe.clone() {
        info!("Stripe billing: enabled (price={})", stripe_config.price_id);
        let stripe_state = diaryx_sync_server::handlers::stripe::StripeState {
            repo: repo.clone(),
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
        // API routes
        .nest("/api", api_routes(api_state))
        .nest("/api", site_routes(sites_state.clone()))
        // Caddy verify-domain endpoint (unauthenticated)
        .nest("/api", verify_domain_route(sites_state))
        // Session routes (for live share)
        .nest("/api/sessions", session_routes(sessions_state))
        // Sync v2 endpoint (siphonophore-based)
        .merge(sync_v2_router);

    // Stripe billing routes (only if configured)
    if let Some(stripe) = stripe_router {
        app = app.nest("/api", stripe);
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
            let _ = cleanup_repo.cleanup_expired_share_sessions();
            info!("Cleaned up expired tokens, sessions, and share sessions");
        }
    });

    // Start attachment blob GC task
    {
        let gc_repo = repo.clone();
        let gc_blob_store = blob_store.clone();
        let retention_days = config.r2.gc_retention_days.max(1);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(3600));
            loop {
                interval.tick().await;
                let cutoff = chrono::Utc::now()
                    .checked_sub_signed(chrono::Duration::days(retention_days))
                    .unwrap_or_else(chrono::Utc::now)
                    .timestamp();
                let due = match gc_repo.list_soft_deleted_blobs_due(cutoff) {
                    Ok(rows) => rows,
                    Err(err) => {
                        error!("Blob GC query failed: {}", err);
                        continue;
                    }
                };

                for row in due {
                    if !row.r2_key.is_empty() {
                        if let Err(err) = gc_blob_store.delete(&row.r2_key).await {
                            error!(
                                "Blob GC delete failed for {} (user {}): {}",
                                row.r2_key, row.user_id, err
                            );
                            continue;
                        }
                    }
                    if let Err(err) = gc_repo.delete_blob_row(&row.user_id, &row.blob_hash) {
                        error!(
                            "Blob GC DB delete failed for {}:{}: {}",
                            row.user_id, row.blob_hash, err
                        );
                    }
                }
            }
        });
    }

    // Start expired attachment upload cleanup task
    {
        let uploads_repo = repo.clone();
        let uploads_blob_store = blob_store.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(600));
            loop {
                interval.tick().await;
                let now = chrono::Utc::now().timestamp();
                let expired = match uploads_repo.list_expired_attachment_uploads(now) {
                    Ok(rows) => rows,
                    Err(err) => {
                        error!("Attachment upload cleanup query failed: {}", err);
                        continue;
                    }
                };

                for session in expired {
                    if !session.r2_multipart_upload_id.is_empty() {
                        if let Err(err) = uploads_blob_store
                            .abort_multipart(&session.r2_key, &session.r2_multipart_upload_id)
                            .await
                        {
                            error!(
                                "Attachment upload abort failed for {}:{}: {}",
                                session.upload_id, session.r2_key, err
                            );
                        }
                    }
                    if let Err(err) =
                        uploads_repo.set_attachment_upload_status(&session.upload_id, "expired")
                    {
                        error!(
                            "Attachment upload status update failed for {}: {}",
                            session.upload_id, err
                        );
                    }
                    if let Err(err) =
                        uploads_repo.delete_attachment_upload_session(&session.upload_id)
                    {
                        error!(
                            "Attachment upload delete failed for {}: {}",
                            session.upload_id, err
                        );
                    }
                }
            }
        });
    }

    // Start git auto-commit background task
    {
        let sync_v2_state = sync_v2_state.clone();
        let quiescence_mins = config.git_quiescence_minutes;
        let max_staleness_hours = config.git_max_staleness_hours;
        let auto_repo = repo.clone();
        let auto_sites_store = sites_store.clone();
        let auto_attachments_store = blob_store.clone();
        let auto_publish_lock = publish_lock.clone();
        info!(
            "Git auto-commit: quiescence={}min, max_staleness={}h",
            quiescence_mins, max_staleness_hours
        );
        tokio::spawn(async move {
            let check_interval = tokio::time::Duration::from_secs(60);
            let quiescence = tokio::time::Duration::from_secs(u64::from(quiescence_mins) * 60);
            let max_staleness =
                tokio::time::Duration::from_secs(u64::from(max_staleness_hours) * 3600);
            let mut interval = tokio::time::interval(check_interval);

            loop {
                interval.tick().await;

                // Collect workspaces that are ready to commit
                let candidates: Vec<String> = {
                    let dirty = sync_v2_state.dirty_workspaces.read().await;
                    let now = tokio::time::Instant::now();
                    dirty
                        .iter()
                        .filter(|(_, last_change)| {
                            let elapsed = now.duration_since(**last_change);
                            elapsed >= quiescence || elapsed >= max_staleness
                        })
                        .map(|(id, _)| id.clone())
                        .collect()
                };

                for workspace_id in candidates {
                    // Skip orphaned workspace IDs that no longer exist in user_workspaces.
                    // Also opportunistically clean up leftover local artifacts.
                    match auto_repo.get_workspace(&workspace_id) {
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            sync_v2_state
                                .dirty_workspaces
                                .write()
                                .await
                                .remove(&workspace_id);

                            sync_v2_state.storage_cache.evict_storage(&workspace_id);

                            let db_path =
                                sync_v2_state.storage_cache.workspace_db_path(&workspace_id);
                            if db_path.exists() {
                                if let Err(err) = std::fs::remove_file(&db_path) {
                                    warn!(
                                        "Failed to remove orphan workspace DB {}: {}",
                                        db_path.display(),
                                        err
                                    );
                                } else {
                                    info!("Removed orphan workspace DB {}", db_path.display());
                                }
                            }

                            let git_path = sync_v2_state.storage_cache.git_repo_path(&workspace_id);
                            if git_path.exists() {
                                if let Err(err) = std::fs::remove_dir_all(&git_path) {
                                    warn!(
                                        "Failed to remove orphan workspace git repo {}: {}",
                                        git_path.display(),
                                        err
                                    );
                                } else {
                                    info!(
                                        "Removed orphan workspace git repo {}",
                                        git_path.display()
                                    );
                                }
                            }

                            warn!(
                                "Skipping auto-commit for unknown workspace {}; cleared dirty state",
                                workspace_id
                            );
                            continue;
                        }
                        Err(err) => {
                            error!(
                                "Failed to verify workspace {} before auto-commit: {}",
                                workspace_id, err
                            );
                            continue;
                        }
                    }

                    // Check peer count — prefer committing when no one is connected
                    let doc_id = format!("workspace:{}", workspace_id);
                    let peer_count = sync_v2_state.handle.get_peer_count(&doc_id).await;

                    // Check if past max staleness (commit even with peers)
                    let force = {
                        let dirty = sync_v2_state.dirty_workspaces.read().await;
                        dirty.get(&workspace_id).is_some_and(|last_change| {
                            tokio::time::Instant::now().duration_since(*last_change)
                                >= max_staleness
                        })
                    };

                    if peer_count > 0 && !force {
                        continue;
                    }

                    // Attempt commit
                    match diaryx_sync_server::git_ops::commit_workspace_by_id(
                        &sync_v2_state.storage_cache,
                        &workspace_id,
                        None,
                    ) {
                        Ok(result) => {
                            sync_v2_state
                                .dirty_workspaces
                                .write()
                                .await
                                .remove(&workspace_id);
                            info!(
                                "Auto-committed workspace {}: {} files [{}]",
                                workspace_id, result.file_count, result.commit_id
                            );

                            let site = match auto_repo.get_site_for_workspace(&workspace_id) {
                                Ok(site) => site,
                                Err(err) => {
                                    error!(
                                        "Failed to query site config for {}: {}",
                                        workspace_id, err
                                    );
                                    None
                                }
                            };

                            if let Some(site) = site {
                                if !site.enabled || !site.auto_publish {
                                    continue;
                                }

                                let now = chrono::Utc::now().timestamp();
                                if site
                                    .last_published_at
                                    .is_some_and(|last| last >= now.saturating_sub(300))
                                {
                                    continue;
                                }

                                if !try_acquire_publish_lock(&auto_publish_lock, &workspace_id)
                                    .await
                                {
                                    continue;
                                }

                                let publish_workspace_id = workspace_id.clone();
                                let publish_site = site.clone();
                                let publish_repo = auto_repo.clone();
                                let publish_sites_store = auto_sites_store.clone();
                                let publish_attachments_store = auto_attachments_store.clone();
                                let publish_storage_cache = sync_v2_state.storage_cache.clone();
                                let publish_lock = auto_publish_lock.clone();

                                tokio::spawn(async move {
                                    let publish_result = publish_workspace_to_r2(
                                        publish_repo.as_ref(),
                                        publish_storage_cache.as_ref(),
                                        publish_sites_store.as_ref(),
                                        publish_attachments_store.as_ref(),
                                        &publish_workspace_id,
                                        &publish_site,
                                    )
                                    .await;

                                    if let Err(err) = publish_result {
                                        error!(
                                            "Auto-publish failed for workspace {} (slug={}): {}",
                                            publish_workspace_id, publish_site.slug, err
                                        );
                                    } else {
                                        info!(
                                            "Auto-publish complete for workspace {} (slug={})",
                                            publish_workspace_id, publish_site.slug
                                        );
                                    }

                                    release_publish_lock(&publish_lock, &publish_workspace_id)
                                        .await;
                                });
                            }
                        }
                        Err(e) => {
                            if e.to_string().contains("No files to commit") {
                                sync_v2_state
                                    .dirty_workspaces
                                    .write()
                                    .await
                                    .remove(&workspace_id);
                                info!(
                                    "Auto-commit skipped for {}: no files to commit; cleared dirty state",
                                    workspace_id
                                );
                            } else {
                                // Keep dirty state for retriable failures.
                                error!("Auto-commit failed for {}: {}", workspace_id, e);
                            }
                        }
                    }
                }
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
