use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode},
};
use diaryx_core::crdt::{BinaryRef, FileMetadata, WorkspaceCrdt};
use diaryx_sync_server::{
    auth::AuthExtractor,
    blob_store::InMemoryBlobStore,
    db::{AuthRepo, WorkspaceAttachmentRefRecord, init_database},
    handlers::api::{ApiState, api_routes},
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
) {
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
        sync_v2: sync_v2_state.clone(),
        blob_store: Arc::new(InMemoryBlobStore::new("diaryx-sync".to_string())),
        snapshot_upload_max_bytes: 1024 * 1024 * 1024,
        attachment_incremental_sync_enabled: true,
        admin_secret: None,
    };

    let app = Router::new()
        .nest("/api", api_routes(api_state))
        .layer(Extension(AuthExtractor::new(repo.clone())));

    (app, repo, sync_v2_state, user_id, workspace_id, token)
}

#[tokio::test]
async fn init_upload_rejects_when_over_user_limit() {
    let (app, repo, _sync_v2_state, user_id, workspace_id, token) = setup();
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
    let (app, repo, _sync_v2_state, user_id, workspace_id, token) = setup();
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

#[tokio::test]
async fn complete_upload_reconciles_workspace_refs() {
    let (app, repo, sync_v2_state, _user_id, workspace_id, token) = setup();

    // Seed workspace metadata with an attachment ref that has no hash yet.
    let storage = sync_v2_state
        .storage_cache
        .get_storage(&workspace_id)
        .expect("workspace storage");
    let workspace = WorkspaceCrdt::load_with_name(storage, format!("workspace:{}", workspace_id))
        .expect("workspace crdt");
    let mut metadata = FileMetadata::with_filename("note.md".to_string(), Some("Note".to_string()));
    metadata.attachments = vec![BinaryRef {
        path: "_attachments/a.png".to_string(),
        source: "local".to_string(),
        hash: String::new(),
        mime_type: "image/png".to_string(),
        size: 3,
        uploaded_at: None,
        deleted: false,
    }];
    workspace
        .set_file("note.md", metadata)
        .expect("set workspace metadata");

    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let init_body = serde_json::json!({
      "attachment_path": "_attachments/a.png",
      "hash": hash,
      "size_bytes": 3,
      "mime_type": "image/png",
      "part_size": 8 * 1024 * 1024,
      "total_parts": 1
    });

    let init_request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/workspaces/{}/attachments/uploads",
            workspace_id
        ))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(init_body.to_string()))
        .expect("init request");
    let init_response = app
        .clone()
        .oneshot(init_request)
        .await
        .expect("init response");
    assert_eq!(init_response.status(), StatusCode::OK);
    let init_bytes = axum::body::to_bytes(init_response.into_body(), usize::MAX)
        .await
        .expect("init body");
    let init_json: Value = serde_json::from_slice(&init_bytes).expect("init json");
    let upload_id = init_json["upload_id"]
        .as_str()
        .expect("upload_id")
        .to_string();

    let part_request = Request::builder()
        .method("PUT")
        .uri(format!(
            "/api/workspaces/{}/attachments/uploads/{}/parts/1",
            workspace_id, upload_id
        ))
        .header("content-type", "application/octet-stream")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(vec![b'a', b'b', b'c']))
        .expect("part request");
    let part_response = app
        .clone()
        .oneshot(part_request)
        .await
        .expect("part response");
    assert_eq!(part_response.status(), StatusCode::OK);

    let complete_body = serde_json::json!({
      "attachment_path": "_attachments/a.png",
      "hash": hash,
      "size_bytes": 3,
      "mime_type": "image/png"
    });
    let complete_request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/workspaces/{}/attachments/uploads/{}/complete",
            workspace_id, upload_id
        ))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(complete_body.to_string()))
        .expect("complete request");
    let complete_response = app
        .clone()
        .oneshot(complete_request)
        .await
        .expect("complete response");
    assert_eq!(complete_response.status(), StatusCode::OK);

    assert!(
        repo.workspace_references_blob(&workspace_id, hash)
            .expect("workspace ref check"),
        "completed upload should be referenced after reconcile"
    );
}

#[tokio::test]
async fn delete_workspace_cleans_dirty_state_and_local_storage_artifacts() {
    let (app, _repo, sync_v2_state, _user_id, workspace_id, token) = setup();

    let storage = sync_v2_state
        .storage_cache
        .get_storage(&workspace_id)
        .expect("workspace storage");
    drop(storage);

    let db_path = sync_v2_state.storage_cache.workspace_db_path(&workspace_id);
    assert!(
        db_path.exists(),
        "workspace DB should exist before delete workspace test"
    );

    let git_path = sync_v2_state.storage_cache.git_repo_path(&workspace_id);
    std::fs::create_dir_all(&git_path).expect("create dummy git dir");
    std::fs::write(git_path.join("HEAD"), b"ref: refs/heads/main\n")
        .expect("create dummy git file");
    assert!(
        git_path.exists(),
        "workspace git repo path should exist before deletion"
    );

    sync_v2_state
        .dirty_workspaces
        .write()
        .await
        .insert(workspace_id.clone(), tokio::time::Instant::now());

    let delete_request = Request::builder()
        .method("DELETE")
        .uri(format!("/api/workspaces/{}", workspace_id))
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("delete request");
    let delete_response = app
        .clone()
        .oneshot(delete_request)
        .await
        .expect("delete response");
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let dirty = sync_v2_state.dirty_workspaces.read().await;
    assert!(
        !dirty.contains_key(&workspace_id),
        "dirty workspace state should be cleared after delete"
    );

    assert!(
        !db_path.exists(),
        "workspace DB file should be removed after delete"
    );
    assert!(
        !git_path.exists(),
        "workspace git repo should be removed after delete"
    );
}
