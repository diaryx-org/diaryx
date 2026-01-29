//! End-to-end sync integration tests.
//!
//! These tests verify the full sync pipeline with real WebSocket connections
//! to an in-memory test server. They test:
//!
//! - Basic push/pull synchronization
//! - Bidirectional sync between multiple clients
//! - Incremental updates
//! - Large workspace handling
//! - Metadata and body content integrity
//! - Empty update detection
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     WebSocket      ┌─────────────────┐     WebSocket     ┌─────────────┐
//! │  Client A   │ ←───────────────→  │   Test Server   │ ←───────────────→ │  Client B   │
//! │ (in-memory) │                    │   (in-memory)   │                   │ (in-memory) │
//! └─────────────┘                    └─────────────────┘                   └─────────────┘
//! ```

use diaryx_core::crdt::{
    BodyDocManager, FileMetadata, SqliteStorage, SyncMessage, UpdateOrigin, WorkspaceCrdt,
    frame_body_message, unframe_body_message,
};
use diaryx_sync_server::sync::SyncState;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::{connect_async, tungstenite::Message};

// =============================================================================
// Test Infrastructure
// =============================================================================

/// Test client with in-memory CRDT storage
struct TestClient {
    workspace_crdt: WorkspaceCrdt,
    body_manager: BodyDocManager,
    #[allow(dead_code)]
    storage: Arc<SqliteStorage>,
}

impl TestClient {
    /// Create a new test client with in-memory storage
    fn new() -> Self {
        let storage =
            Arc::new(SqliteStorage::in_memory().expect("Failed to create in-memory storage"));
        let workspace_crdt = WorkspaceCrdt::new(storage.clone());
        let body_manager = BodyDocManager::new(storage.clone());

        Self {
            workspace_crdt,
            body_manager,
            storage,
        }
    }

    /// Add a test file with metadata and optional body content
    fn add_file(&self, path: &str, title: &str, body: Option<&str>) {
        let filename = path.split('/').last().unwrap_or(path).to_string();
        let metadata = FileMetadata {
            filename,
            title: Some(title.to_string()),
            ..Default::default()
        };
        self.workspace_crdt.set_file(path, metadata).unwrap();

        if let Some(body_content) = body {
            let doc = self.body_manager.get_or_create(path);
            doc.set_body(body_content).unwrap();
        }
    }

    /// Get all files as a map of path -> (metadata, body_content)
    fn get_all_files(&self) -> HashMap<String, (FileMetadata, String)> {
        let files = self.workspace_crdt.list_files();
        files
            .into_iter()
            .filter(|(_, meta)| !meta.deleted)
            .map(|(path, meta)| {
                let body = self
                    .body_manager
                    .get(&path)
                    .map(|doc| doc.get_body())
                    .unwrap_or_default();
                (path, (meta, body))
            })
            .collect()
    }

    /// Get the number of non-deleted files
    fn file_count(&self) -> usize {
        self.workspace_crdt
            .list_files()
            .iter()
            .filter(|(_, meta)| !meta.deleted)
            .count()
    }
}

/// Start a test server with in-memory storage on a random available port
async fn start_test_server() -> (SocketAddr, oneshot::Sender<()>) {
    use axum::{Router, extract::ws::WebSocketUpgrade, routing::get};

    // Create in-memory sync state
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let sync_state = Arc::new(SyncState::new(temp_dir.path().to_path_buf()));

    // Create minimal test router with just /sync endpoint
    let app = Router::new()
        .route(
            "/sync",
            get({
                let sync_state = sync_state.clone();
                move |ws: WebSocketUpgrade, query: axum::extract::Query<TestWsQuery>| {
                    let sync_state = sync_state.clone();
                    async move {
                        let workspace_id = query
                            .doc
                            .clone()
                            .unwrap_or_else(|| "test-workspace".to_string());
                        ws.on_upgrade(move |socket| {
                            handle_test_ws(
                                socket,
                                sync_state,
                                workspace_id,
                                query.multiplexed.unwrap_or(false),
                            )
                        })
                    }
                }
            }),
        )
        .route("/health", get(|| async { "OK" }));

    // Bind to random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // Spawn server task
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
        // Keep temp_dir alive until server shuts down
        drop(temp_dir);
    });

    (addr, shutdown_tx)
}

#[derive(Debug, serde::Deserialize)]
struct TestWsQuery {
    doc: Option<String>,
    multiplexed: Option<bool>,
}

/// Handle WebSocket connection for test server
async fn handle_test_ws(
    socket: axum::extract::ws::WebSocket,
    sync_state: Arc<SyncState>,
    workspace_id: String,
    multiplexed: bool,
) {
    use axum::extract::ws::Message as AxumMessage;

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Get or create room
    let room = sync_state.get_or_create_room(&workspace_id).await;

    if multiplexed {
        // Multiplexed body sync mode
        let mut body_rx = room.subscribe_all_bodies();

        // Handle bidirectional communication
        loop {
            tokio::select! {
                Some(msg) = ws_rx.next() => {
                    match msg {
                        Ok(AxumMessage::Binary(data)) => {
                            // Unframe to get file path
                            if let Some((file_path, sync_msg)) = unframe_body_message(&data) {
                                if let Some(response) = room.handle_body_message(&file_path, &sync_msg).await {
                                    let framed = frame_body_message(&file_path, &response);
                                    if ws_tx.send(AxumMessage::Binary(framed.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(AxumMessage::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }

                result = body_rx.recv() => {
                    match result {
                        Ok((file_path, msg)) => {
                            let framed = frame_body_message(&file_path, &msg);
                            if ws_tx.send(AxumMessage::Binary(framed.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => {}
                    }
                }

                else => break,
            }
        }
    } else {
        // Metadata sync mode
        let mut broadcast_rx = room.subscribe();

        // Send initial state
        let initial_state = room.get_full_state().await;
        if ws_tx
            .send(AxumMessage::Binary(initial_state.into()))
            .await
            .is_err()
        {
            return;
        }

        // Handle bidirectional communication
        loop {
            tokio::select! {
                Some(msg) = ws_rx.next() => {
                    match msg {
                        Ok(AxumMessage::Binary(data)) => {
                            if let Some(response) = room.handle_message(&data).await {
                                if ws_tx.send(AxumMessage::Binary(response.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(AxumMessage::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }

                result = broadcast_rx.recv() => {
                    if let Ok(msg) = result {
                        if ws_tx.send(AxumMessage::Binary(msg.into())).await.is_err() {
                            break;
                        }
                    }
                }

                else => break,
            }
        }

        room.unsubscribe();
    }

    sync_state.maybe_remove_room(&workspace_id).await;
}

/// Sync client metadata to server (push mode)
async fn sync_metadata_push(addr: &SocketAddr, workspace_id: &str, client: &TestClient) -> usize {
    let url = format!("ws://{}/sync?doc={}", addr, workspace_id);
    let (mut ws, _) = connect_async(&url).await.expect("Failed to connect");

    // Receive initial state from server
    let _initial = ws
        .next()
        .await
        .expect("No initial message")
        .expect("WS error");

    // Send our state vector (SyncStep1) to initiate sync
    let sv = client.workspace_crdt.encode_state_vector();
    let step1 = SyncMessage::SyncStep1(sv).encode();
    ws.send(Message::Binary(step1.into()))
        .await
        .expect("Failed to send");

    // Receive server's response (SyncStep2 + SyncStep1)
    let _response = ws.next().await;

    // Send our full state as SyncStep2
    let full_state = client.workspace_crdt.encode_state_as_update();
    let step2 = SyncMessage::SyncStep2(full_state).encode();
    ws.send(Message::Binary(step2.into()))
        .await
        .expect("Failed to send");

    // Brief delay to ensure server processes our update
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    ws.close(None).await.ok();
    client.file_count()
}

/// Sync client metadata from server (pull mode)
async fn sync_metadata_pull(addr: &SocketAddr, workspace_id: &str, client: &TestClient) -> usize {
    let url = format!("ws://{}/sync?doc={}", addr, workspace_id);
    let (mut ws, _) = connect_async(&url).await.expect("Failed to connect");

    // Receive initial state from server (this is a SyncStep2)
    let initial_msg = ws
        .next()
        .await
        .expect("No initial message")
        .expect("WS error");
    if let Message::Binary(data) = initial_msg {
        // Decode and apply the initial state
        let messages = SyncMessage::decode_all(&data).unwrap();
        for msg in messages {
            if let SyncMessage::SyncStep2(update) = msg {
                if !update.is_empty() {
                    client
                        .workspace_crdt
                        .apply_update(&update, UpdateOrigin::Remote)
                        .ok();
                }
            }
        }
    }

    // Send our state vector (SyncStep1) to get any remaining updates
    let sv = client.workspace_crdt.encode_state_vector();
    let step1 = SyncMessage::SyncStep1(sv).encode();
    ws.send(Message::Binary(step1.into()))
        .await
        .expect("Failed to send");

    // Receive and apply any additional updates
    if let Some(Ok(Message::Binary(data))) = ws.next().await {
        let messages = SyncMessage::decode_all(&data).unwrap();
        for msg in messages {
            match msg {
                SyncMessage::SyncStep2(update) if !update.is_empty() => {
                    client
                        .workspace_crdt
                        .apply_update(&update, UpdateOrigin::Remote)
                        .ok();
                }
                _ => {}
            }
        }
    }

    // Send our state to complete bidirectional sync
    let full_state = client.workspace_crdt.encode_state_as_update();
    let step2 = SyncMessage::SyncStep2(full_state).encode();
    ws.send(Message::Binary(step2.into()))
        .await
        .expect("Failed to send");

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    ws.close(None).await.ok();

    client.file_count()
}

/// Sync body content for specific files
async fn sync_bodies(
    addr: &SocketAddr,
    workspace_id: &str,
    client: &TestClient,
    file_paths: &[&str],
    push: bool,
) {
    let url = format!("ws://{}/sync?doc={}&multiplexed=true", addr, workspace_id);
    let (mut ws, _) = connect_async(&url).await.expect("Failed to connect");

    for path in file_paths {
        let doc = client.body_manager.get_or_create(path);

        if push {
            // Send our state vector
            let sv = doc.encode_state_vector();
            let step1 = SyncMessage::SyncStep1(sv).encode();
            let framed = frame_body_message(path, &step1);
            ws.send(Message::Binary(framed.into()))
                .await
                .expect("Failed to send");

            // Receive response
            if let Some(Ok(Message::Binary(data))) = ws.next().await {
                if let Some((_, msg)) = unframe_body_message(&data) {
                    for sync_msg in SyncMessage::decode_all(&msg).unwrap() {
                        if let SyncMessage::SyncStep2(update) = sync_msg {
                            if !update.is_empty() {
                                doc.apply_update(&update, UpdateOrigin::Remote).ok();
                            }
                        }
                    }
                }
            }

            // Send our full state
            let full_state = doc.encode_state_as_update();
            let step2 = SyncMessage::SyncStep2(full_state).encode();
            let framed = frame_body_message(path, &step2);
            ws.send(Message::Binary(framed.into()))
                .await
                .expect("Failed to send");
        } else {
            // Pull mode - send empty state vector to get all content
            let step1 = SyncMessage::SyncStep1(Vec::new()).encode();
            let framed = frame_body_message(path, &step1);
            ws.send(Message::Binary(framed.into()))
                .await
                .expect("Failed to send");

            // Receive and apply server's state
            if let Some(Ok(Message::Binary(data))) = ws.next().await {
                if let Some((_, msg)) = unframe_body_message(&data) {
                    for sync_msg in SyncMessage::decode_all(&msg).unwrap() {
                        if let SyncMessage::SyncStep2(update) = sync_msg {
                            if !update.is_empty() {
                                doc.apply_update(&update, UpdateOrigin::Remote).ok();
                            }
                        }
                    }
                }
            }
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    ws.close(None).await.ok();
}

/// Verify two clients have identical state
fn verify_clients_equal(a: &TestClient, b: &TestClient) {
    let files_a = a.get_all_files();
    let files_b = b.get_all_files();

    assert_eq!(
        files_a.len(),
        files_b.len(),
        "File count mismatch: A has {}, B has {}",
        files_a.len(),
        files_b.len()
    );

    for (path, (meta_a, body_a)) in &files_a {
        let (meta_b, body_b) = files_b
            .get(path)
            .unwrap_or_else(|| panic!("File {} missing from client B", path));

        assert_eq!(
            meta_a.title, meta_b.title,
            "Title mismatch for {}: {:?} vs {:?}",
            path, meta_a.title, meta_b.title
        );

        assert_eq!(
            meta_a.part_of, meta_b.part_of,
            "part_of mismatch for {}",
            path
        );

        assert_eq!(
            meta_a.contents, meta_b.contents,
            "contents mismatch for {}",
            path
        );

        assert_eq!(body_a, body_b, "Body content mismatch for {}", path);
    }
}

// =============================================================================
// Test Cases
// =============================================================================

/// Test 1: Basic push/pull synchronization
///
/// Client A creates files with metadata + body content, pushes to server,
/// then Client B (empty) pulls from server and should have identical files.
#[tokio::test]
async fn test_basic_push_pull() {
    let (addr, shutdown) = start_test_server().await;

    // Client A creates files
    let client_a = TestClient::new();
    client_a.add_file(
        "readme.md",
        "README",
        Some("# Welcome\n\nThis is the readme."),
    );
    client_a.add_file(
        "notes/todo.md",
        "Todo List",
        Some("- [ ] Write tests\n- [ ] Review code"),
    );
    client_a.add_file(
        "notes/ideas.md",
        "Ideas",
        Some("Some creative ideas here..."),
    );

    assert_eq!(client_a.file_count(), 3);

    // Push to server
    let pushed = sync_metadata_push(&addr, "test-workspace", &client_a).await;
    assert_eq!(pushed, 3);

    // Sync bodies
    let file_paths: Vec<&str> = vec!["readme.md", "notes/todo.md", "notes/ideas.md"];
    sync_bodies(&addr, "test-workspace", &client_a, &file_paths, true).await;

    // Client B pulls from server
    let client_b = TestClient::new();
    let pulled = sync_metadata_pull(&addr, "test-workspace", &client_b).await;
    assert_eq!(pulled, 3);

    // Sync bodies for client B
    sync_bodies(&addr, "test-workspace", &client_b, &file_paths, false).await;

    // Verify both clients have identical state
    verify_clients_equal(&client_a, &client_b);

    shutdown.send(()).ok();
}

/// Test 2: Bidirectional sync
///
/// Client A has files {a.md, b.md}, Client B has files {c.md, d.md}.
/// After syncing, both should have all four files.
#[tokio::test]
async fn test_bidirectional_sync() {
    let (addr, shutdown) = start_test_server().await;

    // Client A creates its files
    let client_a = TestClient::new();
    client_a.add_file("a.md", "File A", Some("Content of A"));
    client_a.add_file("b.md", "File B", Some("Content of B"));

    // Client B creates its files
    let client_b = TestClient::new();
    client_b.add_file("c.md", "File C", Some("Content of C"));
    client_b.add_file("d.md", "File D", Some("Content of D"));

    // Client A syncs first (push + pull)
    sync_metadata_push(&addr, "test-workspace", &client_a).await;
    sync_bodies(&addr, "test-workspace", &client_a, &["a.md", "b.md"], true).await;

    // Client B syncs (push + pull)
    sync_metadata_push(&addr, "test-workspace", &client_b).await;
    sync_bodies(&addr, "test-workspace", &client_b, &["c.md", "d.md"], true).await;

    // Client B should now have A's files too
    sync_metadata_pull(&addr, "test-workspace", &client_b).await;
    sync_bodies(&addr, "test-workspace", &client_b, &["a.md", "b.md"], false).await;

    // Client A should now have B's files too
    sync_metadata_pull(&addr, "test-workspace", &client_a).await;
    sync_bodies(&addr, "test-workspace", &client_a, &["c.md", "d.md"], false).await;

    // Both should have all 4 files
    assert_eq!(client_a.file_count(), 4);
    assert_eq!(client_b.file_count(), 4);

    verify_clients_equal(&client_a, &client_b);

    shutdown.send(()).ok();
}

/// Test 3: Incremental update
///
/// Initial sync: Client A pushes "Hello"
/// Client A modifies content to "Hello World"
/// Client A pushes update
/// Client B pulls and should have "Hello World"
#[tokio::test]
async fn test_incremental_update() {
    let (addr, shutdown) = start_test_server().await;

    // Initial sync
    let client_a = TestClient::new();
    client_a.add_file("test.md", "Test", Some("Hello"));

    sync_metadata_push(&addr, "test-workspace", &client_a).await;
    sync_bodies(&addr, "test-workspace", &client_a, &["test.md"], true).await;

    // Client B pulls initial state
    let client_b = TestClient::new();
    sync_metadata_pull(&addr, "test-workspace", &client_b).await;
    sync_bodies(&addr, "test-workspace", &client_b, &["test.md"], false).await;

    let body = client_b.body_manager.get("test.md").map(|d| d.get_body());
    assert_eq!(body, Some("Hello".to_string()));

    // Client A modifies content
    let doc = client_a.body_manager.get_or_create("test.md");
    doc.set_body("Hello World").unwrap();

    // Client A pushes update
    sync_bodies(&addr, "test-workspace", &client_a, &["test.md"], true).await;

    // Client B pulls update
    sync_bodies(&addr, "test-workspace", &client_b, &["test.md"], false).await;

    let updated_body = client_b.body_manager.get("test.md").map(|d| d.get_body());
    assert_eq!(updated_body, Some("Hello World".to_string()));

    shutdown.send(()).ok();
}

/// Test 4: Large workspace
///
/// Client A creates 100+ files with varied content, syncs to server,
/// Client B pulls and all files should sync correctly with no corruption.
#[tokio::test]
async fn test_large_workspace() {
    let (addr, shutdown) = start_test_server().await;

    let client_a = TestClient::new();

    // Create 100 files
    let mut file_paths = Vec::new();
    for i in 0..100 {
        let path = format!("files/file_{:03}.md", i);
        let title = format!("File {}", i);
        let body = format!("This is the content of file {}.\n\nIt has some text.", i);
        client_a.add_file(&path, &title, Some(&body));
        file_paths.push(path);
    }

    assert_eq!(client_a.file_count(), 100);

    // Push metadata
    sync_metadata_push(&addr, "large-workspace", &client_a).await;

    // Push bodies in batches
    let path_refs: Vec<&str> = file_paths.iter().map(|s| s.as_str()).collect();
    sync_bodies(&addr, "large-workspace", &client_a, &path_refs, true).await;

    // Client B pulls
    let client_b = TestClient::new();
    sync_metadata_pull(&addr, "large-workspace", &client_b).await;
    sync_bodies(&addr, "large-workspace", &client_b, &path_refs, false).await;

    assert_eq!(client_b.file_count(), 100);
    verify_clients_equal(&client_a, &client_b);

    shutdown.send(()).ok();
}

/// Test 5: Metadata consistency
///
/// Files with complex metadata: nested contents, part_of links.
/// Verify all metadata fields are preserved after sync.
#[tokio::test]
async fn test_metadata_consistency() {
    let (addr, shutdown) = start_test_server().await;

    let client_a = TestClient::new();

    // Create a hierarchy: index -> child files
    let mut index_meta =
        FileMetadata::with_filename("index.md".to_string(), Some("Index".to_string()));
    index_meta.contents = Some(vec!["child1.md".to_string(), "child2.md".to_string()]);
    index_meta.audience = Some(vec!["public".to_string(), "developers".to_string()]);
    index_meta.description = Some("The main index file".to_string());
    client_a
        .workspace_crdt
        .set_file("index.md", index_meta)
        .unwrap();

    let mut child1 =
        FileMetadata::with_filename("child1.md".to_string(), Some("Child 1".to_string()));
    child1.part_of = Some("index.md".to_string());
    child1.audience = Some(vec!["public".to_string()]);
    client_a
        .workspace_crdt
        .set_file("child1.md", child1)
        .unwrap();

    let mut child2 =
        FileMetadata::with_filename("child2.md".to_string(), Some("Child 2".to_string()));
    child2.part_of = Some("index.md".to_string());
    child2.contents = Some(vec!["grandchild.md".to_string()]);
    client_a
        .workspace_crdt
        .set_file("child2.md", child2)
        .unwrap();

    let mut grandchild =
        FileMetadata::with_filename("grandchild.md".to_string(), Some("Grandchild".to_string()));
    grandchild.part_of = Some("child2.md".to_string());
    client_a
        .workspace_crdt
        .set_file("grandchild.md", grandchild)
        .unwrap();

    // Sync
    sync_metadata_push(&addr, "hierarchy-workspace", &client_a).await;

    let client_b = TestClient::new();
    sync_metadata_pull(&addr, "hierarchy-workspace", &client_b).await;

    // Verify metadata is preserved
    let index_b = client_b.workspace_crdt.get_file("index.md").unwrap();
    assert_eq!(index_b.title, Some("Index".to_string()));
    assert_eq!(
        index_b.contents,
        Some(vec!["child1.md".to_string(), "child2.md".to_string()])
    );
    assert_eq!(
        index_b.audience,
        Some(vec!["public".to_string(), "developers".to_string()])
    );
    assert_eq!(index_b.description, Some("The main index file".to_string()));

    let child1_b = client_b.workspace_crdt.get_file("child1.md").unwrap();
    assert_eq!(child1_b.part_of, Some("index.md".to_string()));

    let child2_b = client_b.workspace_crdt.get_file("child2.md").unwrap();
    assert_eq!(child2_b.contents, Some(vec!["grandchild.md".to_string()]));

    let grandchild_b = client_b.workspace_crdt.get_file("grandchild.md").unwrap();
    assert_eq!(grandchild_b.part_of, Some("child2.md".to_string()));

    shutdown.send(()).ok();
}

/// Test 6: Body content integrity
///
/// Files with special characters, unicode, markdown formatting, and
/// large body content (>100KB) should sync with byte-for-byte accuracy.
#[tokio::test]
async fn test_body_content_integrity() {
    let (addr, shutdown) = start_test_server().await;

    let client_a = TestClient::new();

    // File with unicode and special characters
    client_a.add_file(
        "unicode.md",
        "Unicode Test",
        Some("# Unicode Test 🎉\n\nHello, 世界! مرحبا! שלום!\n\nSpecial chars: <>&\"'\n\nEmoji: 🚀 🎨 💻 🎵"),
    );

    // File with markdown formatting
    client_a.add_file(
        "markdown.md",
        "Markdown Test",
        Some("# Header 1\n\n## Header 2\n\n**Bold** and *italic* and ~~strikethrough~~\n\n```rust\nfn main() {\n    println!(\"Hello!\");\n}\n```\n\n- List item 1\n- List item 2\n  - Nested item\n\n> Blockquote here"),
    );

    // Large file (>100KB)
    let large_content: String = (0..5000)
        .map(|i| {
            format!(
                "Line {}: This is some repeated content to make the file large.\n",
                i
            )
        })
        .collect();
    assert!(large_content.len() > 100_000);
    client_a.add_file("large.md", "Large File", Some(&large_content));

    // Sync
    sync_metadata_push(&addr, "content-workspace", &client_a).await;
    sync_bodies(
        &addr,
        "content-workspace",
        &client_a,
        &["unicode.md", "markdown.md", "large.md"],
        true,
    )
    .await;

    let client_b = TestClient::new();
    sync_metadata_pull(&addr, "content-workspace", &client_b).await;
    sync_bodies(
        &addr,
        "content-workspace",
        &client_b,
        &["unicode.md", "markdown.md", "large.md"],
        false,
    )
    .await;

    // Verify content integrity
    let unicode_a = client_a
        .body_manager
        .get("unicode.md")
        .map(|d| d.get_body())
        .unwrap();
    let unicode_b = client_b
        .body_manager
        .get("unicode.md")
        .map(|d| d.get_body())
        .unwrap();
    assert_eq!(unicode_a, unicode_b, "Unicode content mismatch");
    assert!(unicode_b.contains("世界"));
    assert!(unicode_b.contains("🎉"));

    let md_a = client_a
        .body_manager
        .get("markdown.md")
        .map(|d| d.get_body())
        .unwrap();
    let md_b = client_b
        .body_manager
        .get("markdown.md")
        .map(|d| d.get_body())
        .unwrap();
    assert_eq!(md_a, md_b, "Markdown content mismatch");

    let large_a = client_a
        .body_manager
        .get("large.md")
        .map(|d| d.get_body())
        .unwrap();
    let large_b = client_b
        .body_manager
        .get("large.md")
        .map(|d| d.get_body())
        .unwrap();
    assert_eq!(large_a.len(), large_b.len(), "Large file size mismatch");
    assert_eq!(large_a, large_b, "Large file content mismatch");

    shutdown.send(()).ok();
}

/// Test 7: Empty update detection
///
/// When a client syncs with the same content as the server,
/// no unnecessary updates should be generated (Y.js 2-byte empty check).
#[tokio::test]
async fn test_empty_update_detection() {
    let (addr, shutdown) = start_test_server().await;

    let client_a = TestClient::new();
    client_a.add_file("test.md", "Test", Some("Test content"));

    // Initial sync
    sync_metadata_push(&addr, "empty-update-workspace", &client_a).await;
    sync_bodies(
        &addr,
        "empty-update-workspace",
        &client_a,
        &["test.md"],
        true,
    )
    .await;

    // Get state vector before second sync
    let sv_before = client_a.workspace_crdt.encode_state_vector();

    // Sync again without changes
    sync_metadata_push(&addr, "empty-update-workspace", &client_a).await;

    // State vector should be the same (no new updates)
    let sv_after = client_a.workspace_crdt.encode_state_vector();
    assert_eq!(sv_before, sv_after, "State vector changed unexpectedly");

    shutdown.send(()).ok();
}

/// Test 8: Concurrent modifications
///
/// Two clients modify the same file simultaneously.
/// CRDTs should merge changes correctly.
#[tokio::test]
async fn test_concurrent_modifications() {
    let (addr, shutdown) = start_test_server().await;

    // Both clients start with the same file
    let client_a = TestClient::new();
    client_a.add_file("shared.md", "Shared File", Some("Initial content"));

    sync_metadata_push(&addr, "concurrent-workspace", &client_a).await;
    sync_bodies(
        &addr,
        "concurrent-workspace",
        &client_a,
        &["shared.md"],
        true,
    )
    .await;

    let client_b = TestClient::new();
    sync_metadata_pull(&addr, "concurrent-workspace", &client_b).await;
    sync_bodies(
        &addr,
        "concurrent-workspace",
        &client_b,
        &["shared.md"],
        false,
    )
    .await;

    // Both clients modify - A adds to metadata, B changes body
    let mut meta_a = client_a.workspace_crdt.get_file("shared.md").unwrap();
    meta_a.description = Some("Added by A".to_string());
    client_a
        .workspace_crdt
        .set_file("shared.md", meta_a)
        .unwrap();

    let doc_b = client_b.body_manager.get_or_create("shared.md");
    doc_b.set_body("Modified by B").unwrap();

    // Sync A's changes
    sync_metadata_push(&addr, "concurrent-workspace", &client_a).await;

    // Sync B's changes
    sync_bodies(
        &addr,
        "concurrent-workspace",
        &client_b,
        &["shared.md"],
        true,
    )
    .await;
    sync_metadata_push(&addr, "concurrent-workspace", &client_b).await;

    // Both pull final state
    sync_metadata_pull(&addr, "concurrent-workspace", &client_a).await;
    sync_bodies(
        &addr,
        "concurrent-workspace",
        &client_a,
        &["shared.md"],
        false,
    )
    .await;

    sync_metadata_pull(&addr, "concurrent-workspace", &client_b).await;
    sync_bodies(
        &addr,
        "concurrent-workspace",
        &client_b,
        &["shared.md"],
        false,
    )
    .await;

    // Both should have merged state
    let meta_a_final = client_a.workspace_crdt.get_file("shared.md").unwrap();
    let meta_b_final = client_b.workspace_crdt.get_file("shared.md").unwrap();
    assert_eq!(meta_a_final.description, Some("Added by A".to_string()));
    assert_eq!(meta_b_final.description, Some("Added by A".to_string()));

    let body_a_final = client_a
        .body_manager
        .get("shared.md")
        .map(|d| d.get_body())
        .unwrap();
    let body_b_final = client_b
        .body_manager
        .get("shared.md")
        .map(|d| d.get_body())
        .unwrap();
    assert_eq!(body_a_final, "Modified by B");
    assert_eq!(body_b_final, "Modified by B");

    shutdown.send(()).ok();
}
