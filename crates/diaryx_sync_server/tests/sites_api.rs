use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode},
};
use diaryx_core::crdt::{BodyDocManager, FileMetadata, WorkspaceCrdt};
use diaryx_sync_server::{
    auth::AuthExtractor,
    blob_store::InMemoryBlobStore,
    db::{AuthRepo, init_database},
    handlers::sites::{SitesState, site_routes},
    publish::new_publish_lock,
    sync_v2::{SyncV2Server, SyncV2State},
};
use rusqlite::Connection;
use serde_json::Value;
use std::sync::Arc;
use tower::util::ServiceExt;

fn setup() -> (
    Router,
    Arc<AuthRepo>,
    Arc<SyncV2State>,
    String,
    String,
    String,
    String,
) {
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
        sync_v2: sync_v2_state.clone(),
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

    (
        app,
        repo,
        sync_v2_state,
        workspace_a,
        token_a,
        workspace_b,
        token_b,
    )
}

async fn read_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
}

#[tokio::test]
async fn create_site_and_prevent_global_slug_conflict() {
    let (app, _repo, _sync_state, workspace_a, token_a, workspace_b, token_b) = setup();

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

#[tokio::test]
async fn publish_respects_audience_filtering_for_public_and_private_groups() {
    let (app, _repo, sync_state, workspace_a, token_a, _workspace_b, _token_b) = setup();

    let storage = sync_state
        .storage_cache
        .get_storage(&workspace_a)
        .expect("workspace storage");
    let workspace =
        WorkspaceCrdt::load_with_name(storage.clone(), format!("workspace:{}", workspace_a))
            .expect("workspace crdt");
    let body_docs = BodyDocManager::new(storage);

    let mut root = FileMetadata::with_filename("README.md".to_string(), Some("Root".to_string()));
    root.contents = Some(vec![
        "discussion-fell.md".to_string(),
        "family-only.md".to_string(),
    ]);
    root.audience = Some(vec!["family".to_string(), "ENGL212".to_string()]);
    workspace.set_file("README.md", root).expect("set root");
    body_docs
        .get_or_create(&format!("body:{}/README.md", workspace_a))
        .set_body("Root body")
        .expect("root body");

    let mut discussion = FileMetadata::with_filename(
        "discussion-fell.md".to_string(),
        Some("Discussion".to_string()),
    );
    discussion.part_of = Some("README.md".to_string());
    discussion.audience = Some(vec!["ENGL212".to_string()]);
    workspace
        .set_file("discussion-fell.md", discussion)
        .expect("set discussion");
    body_docs
        .get_or_create(&format!("body:{}/discussion-fell.md", workspace_a))
        .set_body("ENGL212 body")
        .expect("discussion body");

    let mut family_only =
        FileMetadata::with_filename("family-only.md".to_string(), Some("Family".to_string()));
    family_only.part_of = Some("README.md".to_string());
    family_only.audience = Some(vec!["family".to_string()]);
    workspace
        .set_file("family-only.md", family_only)
        .expect("set family file");
    body_docs
        .get_or_create(&format!("body:{}/family-only.md", workspace_a))
        .set_body("Family body")
        .expect("family body");

    let create_site = Request::builder()
        .method("POST")
        .uri(format!("/api/workspaces/{}/site", workspace_a))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::from(
            serde_json::json!({"slug":"my-family","enabled":true,"auto_publish":true}).to_string(),
        ))
        .expect("create site request");
    let create_site_response = app
        .clone()
        .oneshot(create_site)
        .await
        .expect("create response");
    assert_eq!(create_site_response.status(), StatusCode::CREATED);

    let publish = Request::builder()
        .method("POST")
        .uri(format!("/api/workspaces/{}/site/publish", workspace_a))
        .header("authorization", format!("Bearer {}", token_a))
        .body(Body::empty())
        .expect("publish request");
    let publish_response = app
        .clone()
        .oneshot(publish)
        .await
        .expect("publish response");
    assert_eq!(publish_response.status(), StatusCode::OK);
    let payload = read_json(publish_response).await;

    let audiences = payload["audiences"].as_array().expect("audiences array");
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for item in audiences {
        let name = item["name"].as_str().expect("audience name").to_string();
        let file_count = item["file_count"].as_i64().expect("file_count");
        counts.insert(name, file_count);
    }

    assert_eq!(counts.get("public").copied(), Some(0));
    assert!(counts.get("family").copied().unwrap_or(0) > 0);
    assert!(counts.get("engl212").copied().unwrap_or(0) > 0);
}
