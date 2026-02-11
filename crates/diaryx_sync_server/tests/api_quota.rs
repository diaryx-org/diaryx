use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode},
};
use diaryx_sync_server::{
    auth::AuthExtractor,
    blob_store::InMemoryBlobStore,
    db::{AuthRepo, WorkspaceAttachmentRefRecord, init_database},
    handlers::api::{ApiState, api_routes},
    sync_v2::SyncV2Server,
};
use rusqlite::Connection;
use serde_json::Value;
use std::sync::Arc;
use tower::util::ServiceExt;

fn setup() -> (Router, Arc<AuthRepo>, String, String, String) {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_database(&conn).expect("init db");
    let repo = Arc::new(AuthRepo::new(conn));

    let user_id = repo
        .get_or_create_user("quota@example.com")
        .expect("create user");
    let device_id = repo
        .create_device(&user_id, Some("test-device"), None)
        .expect("create device");
    let token = repo
        .create_session(
            &user_id,
            &device_id,
            chrono::Utc::now() + chrono::Duration::days(1),
        )
        .expect("create token");
    let workspace_id = repo
        .get_or_create_workspace(&user_id, "default")
        .expect("workspace");

    let workspaces_dir =
        std::env::temp_dir().join(format!("diaryx-sync-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspaces_dir).expect("create workspace temp dir");
    let sync_v2_server = SyncV2Server::new(repo.clone(), workspaces_dir);
    let sync_v2_state = Arc::new(sync_v2_server.state());

    let api_state = ApiState {
        repo: repo.clone(),
        sync_v2: sync_v2_state,
        blob_store: Arc::new(InMemoryBlobStore::new("diaryx-sync".to_string())),
        snapshot_upload_max_bytes: 1024 * 1024 * 1024,
        attachment_incremental_sync_enabled: true,
    };

    let app = Router::new()
        .nest("/api", api_routes(api_state))
        .layer(Extension(AuthExtractor::new(repo.clone())));

    (app, repo, user_id, workspace_id, token)
}

#[tokio::test]
async fn init_upload_rejects_when_over_user_limit() {
    let (app, repo, user_id, workspace_id, token) = setup();
    repo.set_user_attachment_limit(&user_id, Some(10))
        .expect("set tiny limit");

    let body = serde_json::json!({
      "attachment_path": "_attachments/a.png",
      "hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "size_bytes": 100,
      "mime_type": "image/png",
      "part_size": 8 * 1024 * 1024,
      "total_parts": 1
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/workspaces/{}/attachments/uploads",
            workspace_id
        ))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(body.to_string()))
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["error"], "storage_limit_exceeded");
}

#[tokio::test]
async fn storage_endpoint_reports_limit_and_over_limit() {
    let (app, repo, user_id, workspace_id, token) = setup();
    repo.set_user_attachment_limit(&user_id, Some(100))
        .expect("set limit");
    repo.upsert_blob(&user_id, "hash-a", "r2-key-a", 200, "image/png")
        .expect("upsert blob");
    repo.replace_workspace_attachment_refs(
        &workspace_id,
        &[WorkspaceAttachmentRefRecord {
            file_path: "README.md".to_string(),
            attachment_path: "_attachments/a.png".to_string(),
            blob_hash: "hash-a".to_string(),
            size_bytes: 200,
            mime_type: "image/png".to_string(),
        }],
    )
    .expect("refs");

    let request = Request::builder()
        .method("GET")
        .uri("/api/user/storage")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["limit_bytes"], 100);
    assert_eq!(json["over_limit"], true);
}
