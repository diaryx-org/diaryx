use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode},
};
use diaryx_sync_server::{
    auth::AuthExtractor,
    blob_store::InMemoryBlobStore,
    db::{AuthRepo, init_database},
    handlers::sites::{SitesState, site_routes},
    publish::new_publish_lock,
    sync_v2::SyncV2Server,
};
use rusqlite::Connection;
use std::sync::Arc;
use tower::util::ServiceExt;

fn setup() -> (Router, Arc<AuthRepo>, String, String, String, String) {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_database(&conn).expect("init db");
    let repo = Arc::new(AuthRepo::new(conn));

    let user_a = repo.get_or_create_user("sites-a@example.com").unwrap();
    let user_b = repo.get_or_create_user("sites-b@example.com").unwrap();

    let device_a = repo.create_device(&user_a, Some("a"), None).unwrap();
    let device_b = repo.create_device(&user_b, Some("b"), None).unwrap();

    let token_a = repo
        .create_session(
            &user_a,
            &device_a,
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .unwrap();
    let token_b = repo
        .create_session(
            &user_b,
            &device_b,
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .unwrap();

    let workspace_a = repo.get_or_create_workspace(&user_a, "default").unwrap();
    let workspace_b = repo.get_or_create_workspace(&user_b, "default").unwrap();

    let workspaces_dir =
        std::env::temp_dir().join(format!("diaryx-sites-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspaces_dir).expect("create workspace temp dir");
    let sync_v2_server = SyncV2Server::new(repo.clone(), workspaces_dir);
    let sync_v2_state = Arc::new(sync_v2_server.state());

    let sites_state = SitesState {
        repo: repo.clone(),
        sync_v2: sync_v2_state,
        sites_store: Arc::new(InMemoryBlobStore::new("diaryx-sync")),
        attachments_store: Arc::new(InMemoryBlobStore::new("diaryx-sync")),
        token_signing_key: vec![7; 32],
        site_limit: 1,
        sites_base_url: "https://sites.example.com".to_string(),
        publish_lock: new_publish_lock(),
    };

    let app = Router::new()
        .nest("/api", site_routes(sites_state))
        .layer(Extension(AuthExtractor::new(repo.clone())));

    (app, repo, workspace_a, token_a, workspace_b, token_b)
}

#[tokio::test]
async fn create_site_and_prevent_global_slug_conflict() {
    let (app, _repo, workspace_a, token_a, workspace_b, token_b) = setup();

    let create_a = Request::builder()
        .method("POST")
        .uri(format!("/api/workspaces/{}/site", workspace_a))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::from(
            serde_json::json!({"slug":"my-site","enabled":true,"auto_publish":true}).to_string(),
        ))
        .unwrap();
    let create_a_response = app.clone().oneshot(create_a).await.unwrap();
    assert_eq!(create_a_response.status(), StatusCode::CREATED);

    let create_b_same_slug = Request::builder()
        .method("POST")
        .uri(format!("/api/workspaces/{}/site", workspace_b))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token_b))
        .body(Body::from(
            serde_json::json!({"slug":"my-site","enabled":true,"auto_publish":true}).to_string(),
        ))
        .unwrap();
    let create_b_response = app.clone().oneshot(create_b_same_slug).await.unwrap();
    assert_eq!(create_b_response.status(), StatusCode::CONFLICT);
}
