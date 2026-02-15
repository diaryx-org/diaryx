//! `diaryx edit` command handler.
//!
//! Starts a local sync server and opens the Diaryx web app for rich editing.

use diaryx_core::crdt::{BodyDoc, CrdtStorage, FileMetadata, SqliteStorage, WorkspaceCrdt};
use diaryx_core::frontmatter;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::workspace::Workspace;
use diaryx_sync::local::{create_local_router, generate_session_code};
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Run the edit command: start local sync server and open browser.
pub async fn handle_edit(workspace_root: &Path, url: Option<String>, port: Option<u16>) -> bool {
    let target_url = url.unwrap_or_else(|| "https://app.diaryx.org".to_string());

    // Use workspace directory name as workspace ID
    let workspace_id = workspace_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default")
        .to_string();

    // Generate session code
    let session_code = generate_session_code();

    // Ensure CRDT storage is populated from the workspace files
    let db_path = workspace_root.join(format!("{}.db", workspace_id));
    if let Err(e) = initialize_crdt_from_workspace(workspace_root, &workspace_id, &db_path).await {
        eprintln!("Warning: Failed to initialize CRDT storage: {}", e);
        eprintln!("The web editor may show an empty workspace.");
    }

    // Create the local sync server router
    let router = create_local_router(
        workspace_root.to_path_buf(),
        workspace_id.clone(),
        session_code.clone(),
    );

    // Bind to the requested port (or auto-select)
    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind to {}: {}", addr, e);
            return false;
        }
    };

    let bound_addr = listener.local_addr().unwrap();
    let sync_url = format!("ws://localhost:{}", bound_addr.port());

    // Build the browser URL
    let browser_url = format!(
        "{}?sync_url={}&join_code={}",
        target_url,
        urlencoding::encode(&sync_url),
        urlencoding::encode(&session_code),
    );

    println!("Starting local sync server on {}", bound_addr);
    println!("Session code: {}", session_code);
    println!("Opening: {}", browser_url);
    println!();
    println!("Press Ctrl+C to stop.");

    // Open the browser
    if let Err(e) = open::that(&browser_url) {
        eprintln!("Failed to open browser: {}", e);
        eprintln!("Please open the URL manually: {}", browser_url);
    }

    // Serve until shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    println!("\nLocal sync server stopped.");
    true
}

/// Initialize CRDT storage from workspace files so the web editor sees them.
///
/// Uses `Workspace::collect_workspace_files` to walk the tree from the root index,
/// following `contents` references. Only files reachable from the root index are included.
async fn initialize_crdt_from_workspace(
    workspace_root: &Path,
    workspace_id: &str,
    db_path: &Path,
) -> Result<(), String> {
    // Remove any stale database from a previous session so we always
    // reflect the current filesystem state. Must also remove WAL/SHM
    // files or SQLite will SIGBUS trying to use them without the main DB.
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));
    }

    let storage =
        SqliteStorage::open(db_path).map_err(|e| format!("Failed to open storage: {}", e))?;
    let storage = Arc::new(storage);

    let doc_name = format!("workspace:{}", workspace_id);
    let workspace_crdt = WorkspaceCrdt::load_with_name(storage.clone(), doc_name)
        .map_err(|e| format!("Failed to load workspace CRDT: {}", e))?;

    // Use the core Workspace to find root index and collect reachable files
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let root_index = ws
        .find_root_index_in_dir(workspace_root)
        .await
        .map_err(|e| format!("Failed to scan for root index: {}", e))?
        .ok_or(
            "No root index found. A root index is a .md file with `contents` but no `part_of`.",
        )?;

    let files = ws
        .collect_workspace_files(&root_index)
        .await
        .map_err(|e| format!("Failed to collect workspace files: {}", e))?;

    // Populate CRDT storage for each reachable file
    for file_path in &files {
        if let Err(e) = populate_crdt_for_file(
            workspace_root,
            file_path,
            &workspace_crdt,
            &storage,
            workspace_id,
        ) {
            eprintln!("Warning: skipping {:?}: {}", file_path, e);
        }
    }

    // Save a base snapshot so the sync server can load it via load_doc().
    workspace_crdt
        .save()
        .map_err(|e| format!("Failed to save workspace snapshot: {}", e))?;

    println!(
        "Initialized CRDT storage with {} files from root index {:?}.",
        files.len(),
        root_index.file_name().unwrap_or_default()
    );
    Ok(())
}

/// Populate workspace CRDT metadata and body doc for a single file.
fn populate_crdt_for_file(
    workspace_root: &Path,
    file_path: &Path,
    workspace_crdt: &WorkspaceCrdt,
    storage: &Arc<SqliteStorage>,
    workspace_id: &str,
) -> Result<(), String> {
    let rel_path = file_path
        .strip_prefix(workspace_root)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .replace('\\', "/");

    let content =
        std::fs::read_to_string(file_path).map_err(|e| format!("Failed to read: {}", e))?;

    let parsed =
        frontmatter::parse_or_empty(&content).map_err(|e| format!("Failed to parse: {}", e))?;

    let mut metadata = FileMetadata::from_frontmatter(&parsed.frontmatter);

    metadata.filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    if metadata.title.is_none() {
        metadata.title = Some(
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string(),
        );
    }

    let _ = workspace_crdt.set_file(&rel_path, metadata);

    // Store body content as a proper Y.js document
    let body_key = format!("body:{}/{}", workspace_id, rel_path);
    let body = frontmatter::extract_body(&content);
    let body_storage: Arc<dyn CrdtStorage> = storage.clone();
    let body_doc = BodyDoc::new(body_storage, body_key);
    let _ = body_doc.set_body(body);
    let _ = body_doc.save();

    Ok(())
}

/// Wait for Ctrl+C or termination signal.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install Ctrl+C handler");
}
