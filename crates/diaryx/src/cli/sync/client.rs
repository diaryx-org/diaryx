//! Sync client command handlers.
//!
//! Handles start, push, and pull commands using WebSocket connections.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine;
use diaryx_core::config::Config;
use diaryx_core::crdt::{
    BodyDocManager, DocIdKind, RustSyncManager, SyncHandler, SyncMessage, WorkspaceCrdt,
    format_body_doc_id, format_workspace_doc_id, frame_message_v2, parse_doc_id,
    unframe_message_v2,
};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::CrdtContext;
use super::progress;

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// Scan the workspace and import existing files into the CRDT.
///
/// This is needed for first-time sync when local files exist but the CRDT is empty.
fn import_existing_files(
    workspace_root: &Path,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
) -> usize {
    use diaryx_core::crdt::FileMetadata;
    use std::fs;

    let mut imported = 0;

    // Walk the workspace directory
    fn walk_dir(
        dir: &Path,
        workspace_root: &Path,
        workspace_crdt: &WorkspaceCrdt,
        body_manager: &BodyDocManager,
        imported: &mut usize,
    ) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Skip hidden files/directories and .diaryx
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }

            if path.is_dir() {
                walk_dir(
                    &path,
                    workspace_root,
                    workspace_crdt,
                    body_manager,
                    imported,
                );
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                // Get relative path from workspace root (always use forward slashes for cross-platform consistency)
                let rel_path = match path.strip_prefix(workspace_root) {
                    Ok(p) => p
                        .iter()
                        .map(|c| c.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                    Err(_) => continue,
                };

                // Skip if already in CRDT
                if workspace_crdt.get_file(&rel_path).is_some() {
                    continue;
                }

                // Read and parse the file
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let parsed = match diaryx_core::frontmatter::parse_or_empty(&content) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                // Extract metadata from frontmatter
                let fm = &parsed.frontmatter;
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let metadata = FileMetadata {
                    filename,
                    title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
                    part_of: fm.get("part_of").and_then(|v| v.as_str()).map(String::from),
                    contents: fm.get("contents").and_then(|v| {
                        v.as_sequence().map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                    }),
                    attachments: vec![],
                    deleted: false,
                    audience: fm.get("audience").and_then(|v| {
                        v.as_sequence().map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                    }),
                    description: fm
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    extra: std::collections::HashMap::new(),
                    modified_at: chrono::Utc::now().timestamp_millis(),
                };

                // Add to workspace CRDT
                if workspace_crdt.set_file(&rel_path, metadata).is_ok() {
                    // Also initialize body doc with content
                    let body_doc = body_manager.get_or_create(&rel_path);
                    let _ = body_doc.set_body(&parsed.body);
                    *imported += 1;

                    if *imported % 10 == 0 {
                        print!("\r\x1b[K  Importing local files... {}", imported);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                }
            }
        }
    }

    walk_dir(
        workspace_root,
        workspace_root,
        workspace_crdt,
        body_manager,
        &mut imported,
    );

    if imported > 0 {
        println!("\r\x1b[K  Imported {} local files into CRDT", imported);
    }

    imported
}

/// Control message from the sync server (JSON over WebSocket text frames).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ControlMessage {
    /// Sync progress update from server
    SyncProgress { completed: usize, total: usize },
    /// Initial sync has completed
    SyncComplete { files_synced: usize },
    /// A peer joined the sync session
    PeerJoined {
        #[serde(default)]
        peer_count: usize,
    },
    /// A peer left the sync session
    PeerLeft {
        #[serde(default)]
        peer_count: usize,
    },
    /// Focus list changed - files that any client is focused on
    FocusListChanged { files: Vec<String> },
    /// Files-Ready handshake: server sends file manifest before y-sync starts.
    FileManifest {
        #[serde(default)]
        files: Vec<serde_json::Value>,
        #[serde(default)]
        client_is_new: bool,
    },
    /// Files-Ready handshake: server sends CRDT state after client replies with FilesReady.
    CrdtState {
        /// Base64-encoded Y-CRDT state bytes.
        state: String,
    },
    /// Share session: guest joined confirmation.
    #[serde(alias = "session_joined")]
    SessionJoined {},
    /// Catch-all for other message types
    #[serde(other)]
    Other,
}

/// Handle the start command - start continuous sync.
pub fn handle_start(config: &Config, workspace_root: &Path) {
    // Validate configuration
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or_else(|| {
        // Generate a new workspace ID if not set
        // In a real implementation, this would be assigned by the server
        "default"
    });

    println!("Starting sync...");
    println!("  Server: {}", server_url);
    println!("  Workspace: {}", workspace_id);
    println!("  Local path: {}", workspace_root.display());
    println!();

    // Initialize CRDT context
    let ctx = match CrdtContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let workspace_crdt = ctx.workspace_crdt;
    let body_manager = ctx.body_manager;

    // Check if CRDT is empty and import existing files
    let existing_files = workspace_crdt.list_files();
    if existing_files.is_empty() {
        println!("  Scanning local files...");
        progress::show_indeterminate();
        let imported = import_existing_files(workspace_root, &workspace_crdt, &body_manager);
        if imported > 0 {
            println!("  Ready to sync {} files", imported);
        }
    } else {
        println!("  CRDT has {} files tracked", existing_files.len());
    }

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let sync_manager = Arc::new(RustSyncManager::new(
        Arc::clone(&workspace_crdt),
        Arc::clone(&body_manager),
        Arc::clone(&sync_handler),
    ));

    // Build WebSocket URL for v2 protocol (single connection)
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let sync_url = format!("{}/sync2?token={}", ws_server, session_token);

    // Set up shutdown flag
    let running = Arc::new(AtomicBool::new(true));

    // Ensure progress bar is cleared on exit
    let _progress_guard = progress::ProgressGuard::new();

    // Show connecting state
    progress::show_indeterminate();

    // Run the sync loop
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        let running_clone = running.clone();

        // Set up Ctrl+C handler inside the async context
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    println!("\nShutting down sync...");
                    progress::hide();
                    running_clone.store(false, Ordering::SeqCst);
                }
                Err(e) => {
                    eprintln!("Failed to listen for Ctrl+C: {}", e);
                }
            }
        });

        run_sync_loop_v2(
            &sync_url,
            workspace_id,
            sync_manager,
            workspace_crdt,
            running,
        )
        .await;
    });

    println!("Sync stopped.");
}

/// Handle the push command - one-shot push of local changes.
pub fn handle_push(config: &Config, workspace_root: &Path) {
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or("default");

    println!("Pushing local changes...");

    // Initialize CRDT context
    let ctx = match CrdtContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let workspace_crdt = ctx.workspace_crdt;
    let body_manager = ctx.body_manager;

    // Import existing files if CRDT is empty
    let existing_files = workspace_crdt.list_files();
    if existing_files.is_empty() {
        println!("  Scanning local files...");
        let imported = import_existing_files(workspace_root, &workspace_crdt, &body_manager);
        if imported == 0 {
            println!("No files found to push.");
            return;
        }
        println!("  Found {} files to push", imported);
    } else {
        println!("  {} files in local CRDT", existing_files.len());
    }

    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let sync_manager = Arc::new(RustSyncManager::new(
        Arc::clone(&workspace_crdt),
        Arc::clone(&body_manager),
        Arc::clone(&sync_handler),
    ));

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let sync_url = format!("{}/sync2?token={}", ws_server, session_token);

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match do_one_shot_sync_v2(
            &sync_url,
            workspace_id,
            &sync_manager,
            &workspace_crdt,
            &body_manager,
            false,
        )
        .await
        {
            Ok(0) => println!("  Already up to date"),
            Ok(count) => println!("  Pushed {} items", count),
            Err(e) => eprintln!("  Failed to push: {}", e),
        }
    });

    println!("Push complete.");
}

/// Handle the pull command - one-shot pull of remote changes.
pub fn handle_pull(config: &Config, workspace_root: &Path) {
    let Some(session_token) = &config.sync_session_token else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = config
        .sync_server_url
        .as_deref()
        .unwrap_or(DEFAULT_SYNC_SERVER);

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or("default");

    println!("Pulling remote changes...");

    // Initialize CRDT context
    let ctx = match CrdtContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let workspace_crdt = ctx.workspace_crdt;
    let body_manager = ctx.body_manager;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let sync_manager = Arc::new(RustSyncManager::new(
        Arc::clone(&workspace_crdt),
        Arc::clone(&body_manager),
        Arc::clone(&sync_handler),
    ));

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let sync_url = format!("{}/sync2?token={}", ws_server, session_token);

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match do_one_shot_sync_v2(
            &sync_url,
            workspace_id,
            &sync_manager,
            &workspace_crdt,
            &body_manager,
            true,
        )
        .await
        {
            Ok(0) => println!("  Already up to date"),
            Ok(count) => {
                println!("  Received {} items", count);

                // Write updated files to disk
                let files = workspace_crdt.list_files();
                if !files.is_empty() {
                    let body_mgr_ref = Some(body_manager.as_ref());
                    if let Err(e) = sync_handler
                        .handle_remote_metadata_update(files, vec![], body_mgr_ref, true)
                        .await
                    {
                        eprintln!("  Warning: Failed to write some files: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("  Failed to pull: {}", e),
        }
    });

    println!("Pull complete.");
}

// ===========================================================================
// v2 Protocol (Siphonophore) - Single WebSocket Connection
// ===========================================================================

/// Run the v2 sync loop with a single WebSocket connection to /sync2.
///
/// This uses the siphonophore wire format:
/// - Binary messages: `[u8: doc_id_len] [doc_id] [y-sync payload]`
/// - Text messages: JSON control messages (unchanged)
///
/// Doc ID format:
/// - Workspace: `workspace:{workspace_id}`
/// - Body: `body:{workspace_id}/{file_path}`
async fn run_sync_loop_v2(
    url: &str,
    workspace_id: &str,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    workspace_crdt: Arc<WorkspaceCrdt>,
    running: Arc<AtomicBool>,
) {
    println!("Connecting to sync server (v2 protocol)...");
    progress::show_progress(10);

    // Connect to single /sync2 WebSocket
    let mut ws = match connect_async(url).await {
        Ok((ws, _)) => {
            println!("Connected to sync server");
            progress::show_progress(30);
            ws
        }
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
            progress::show_error(30);
            return;
        }
    };

    // Send workspace SyncStep1 (triggers server's on_before_sync handshake)
    let ws_doc_id = format_workspace_doc_id(workspace_id);
    let ws_step1 = sync_manager.create_workspace_sync_step1();
    let ws_framed = frame_message_v2(&ws_doc_id, &ws_step1);
    if let Err(e) = ws.send(Message::Binary(ws_framed.into())).await {
        eprintln!("Failed to send workspace SyncStep1: {}", e);
        return;
    }

    // Wait for Files-Ready handshake (if server requires it).
    // The server sends file_manifest → we reply FilesReady → server sends crdt_state.
    // If no files exist on server, it returns Continue (no handshake) and we get a
    // binary SyncStep2 directly.
    // Stash any binary message received during handshake to process later.
    let mut stashed_binary: Option<Vec<u8>> = None;
    let handshake_timeout = tokio::time::Duration::from_secs(10);
    let handshake_deadline = tokio::time::Instant::now() + handshake_timeout;

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            match ctrl_msg {
                                ControlMessage::FileManifest { .. } => {
                                    println!("  Received file manifest, completing handshake...");
                                    let files_ready = r#"{"type":"FilesReady"}"#;
                                    if let Err(e) = ws.send(Message::Text(files_ready.into())).await {
                                        eprintln!("Failed to send FilesReady: {}", e);
                                        return;
                                    }
                                }
                                ControlMessage::CrdtState { state } => {
                                    match base64::engine::general_purpose::STANDARD.decode(&state) {
                                        Ok(state_bytes) => {
                                            match sync_manager.handle_crdt_state(&state_bytes).await {
                                                Ok(count) => println!("  Applied CRDT state ({} files)", count),
                                                Err(e) => eprintln!("  Warning: Failed to apply CRDT state: {}", e),
                                            }
                                        }
                                        Err(e) => eprintln!("  Warning: Failed to decode CRDT state: {}", e),
                                    }
                                    break; // Handshake complete
                                }
                                ControlMessage::SessionJoined { .. } => {
                                    println!("  Session joined");
                                    // Continue waiting for file_manifest
                                }
                                _ => {} // Ignore other messages during handshake
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // Server returned Continue (no handshake) — got a binary sync response.
                        stashed_binary = Some(data.to_vec());
                        break;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        eprintln!("Connection closed during handshake");
                        return;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(handshake_deadline) => {
                println!("  No handshake required (timeout), proceeding with sync...");
                break;
            }
        }
    }

    // Send body SyncStep1 for all known files
    let files = workspace_crdt.list_files();
    let file_count = files.len();
    let mut sent = 0;

    const BATCH_SIZE: usize = 50;
    for (file_path, _metadata) in files {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        let body_doc_id = format_body_doc_id(workspace_id, &file_path);
        let body_step1 = sync_manager.create_body_sync_step1(&file_path);
        let body_framed = frame_message_v2(&body_doc_id, &body_step1);
        if let Err(e) = ws.send(Message::Binary(body_framed.into())).await {
            eprintln!("Failed to send body SyncStep1 for {}: {}", file_path, e);
        }
        sent += 1;

        if sent % BATCH_SIZE == 0 {
            print!("\r\x1b[K  Sending state: {}/{} files", sent, file_count);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    if sent > 0 {
        println!("\r\x1b[K  Sent state for {} files", sent);
    }

    progress::show_progress(50);
    println!();
    println!("Sync is running. Press Ctrl+C to stop.");
    println!();

    // Track sync completion
    let workspace_synced = Arc::new(AtomicBool::new(false));
    let body_synced = Arc::new(AtomicBool::new(file_count == 0));

    // Process any binary message that arrived during handshake
    if let Some(data) = stashed_binary.take() {
        if let Some((doc_id, payload)) = unframe_message_v2(&data) {
            match parse_doc_id(&doc_id) {
                Some(DocIdKind::Workspace(_)) => {
                    if let Ok(result) = sync_manager.handle_workspace_message(&payload, true).await
                    {
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            let _ = ws.send(Message::Binary(framed.into())).await;
                        }
                        if !result.changed_files.is_empty() {
                            for file in &result.changed_files {
                                println!("  Synced: {}", file);
                            }
                        }
                    }
                }
                Some(DocIdKind::Body { file_path, .. }) => {
                    if let Ok(result) = sync_manager
                        .handle_body_message(&file_path, &payload, true)
                        .await
                    {
                        if let Some(response) = result.response {
                            let framed = frame_message_v2(&doc_id, &response);
                            let _ = ws.send(Message::Binary(framed.into())).await;
                        }
                    }
                }
                None => {}
            }
        }
    }

    // Message loop
    while running.load(Ordering::SeqCst) {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        // Unframe v2 message
                        if let Some((doc_id, payload)) = unframe_message_v2(&data) {
                            match parse_doc_id(&doc_id) {
                                Some(DocIdKind::Workspace(_)) => {
                                    // Handle workspace message
                                    match sync_manager.handle_workspace_message(&payload, true).await {
                                        Ok(result) => {
                                            if let Some(response) = result.response {
                                                let framed = frame_message_v2(&doc_id, &response);
                                                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                                                    eprintln!("Failed to send workspace response: {}", e);
                                                }
                                            }
                                            if !result.changed_files.is_empty() {
                                                for file in &result.changed_files {
                                                    println!("  Synced: {}", file);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error handling workspace message: {}", e);
                                        }
                                    }
                                }
                                Some(DocIdKind::Body { file_path, .. }) => {
                                    // Handle body message
                                    match sync_manager.handle_body_message(&file_path, &payload, true).await {
                                        Ok(result) => {
                                            if let Some(response) = result.response {
                                                let framed = frame_message_v2(&doc_id, &response);
                                                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                                                    eprintln!("Failed to send body response: {}", e);
                                                }
                                            }
                                            if result.content.is_some() && !result.is_echo {
                                                println!("\r\x1b[K  Body synced: {}", file_path);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error handling body message for {}: {}", file_path, e);
                                        }
                                    }
                                }
                                None => {
                                    log::debug!("Unknown doc_id format: {}", doc_id);
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        // Handle JSON control messages
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            match ctrl_msg {
                                ControlMessage::SyncProgress { completed, total } => {
                                    if total > 0 {
                                        let percent = ((completed as f64 / total as f64) * 100.0) as u8;
                                        let scaled = 50 + (percent / 2);
                                        progress::show_progress(scaled);
                                        print!("\r\x1b[K  Progress: {}/{} files ({}%)", completed, total, percent);
                                        use std::io::Write;
                                        let _ = std::io::stdout().flush();
                                    }
                                }
                                ControlMessage::SyncComplete { files_synced } => {
                                    workspace_synced.store(true, Ordering::SeqCst);
                                    body_synced.store(true, Ordering::SeqCst);
                                    println!("\r\x1b[K  Sync complete ({} files)", files_synced);
                                    progress::show_progress(100);
                                    println!("Watching for changes...");
                                    progress::show_indeterminate();
                                }
                                ControlMessage::PeerJoined { peer_count } => {
                                    println!("\r\x1b[K  Peer joined ({} connected)", peer_count);
                                }
                                ControlMessage::PeerLeft { peer_count } => {
                                    println!("\r\x1b[K  Peer left ({} connected)", peer_count);
                                }
                                ControlMessage::FocusListChanged { files } => {
                                    if !files.is_empty() {
                                        log::debug!("Focus list changed: {} files", files.len());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        println!("\r\x1b[KConnection closed by server");
                        break;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // Connection alive
                    }
                    Some(Err(e)) => {
                        eprintln!("\r\x1b[KWebSocket error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                // Send ping to keep connection alive
                if let Err(e) = ws.send(Message::Ping(vec![].into())).await {
                    eprintln!("Failed to send ping: {}", e);
                    break;
                }
            }
        }
    }

    // Close connection gracefully
    let _ = ws.close(None).await;
}

/// Perform one-shot v2 sync (push or pull).
async fn do_one_shot_sync_v2(
    url: &str,
    workspace_id: &str,
    sync_manager: &RustSyncManager<SyncToAsyncFs<RealFileSystem>>,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
    pull: bool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    use std::collections::HashSet;

    let (mut ws, _) = connect_async(url).await?;

    // Send workspace SyncStep1 (triggers server's on_before_sync handshake)
    let ws_doc_id = format_workspace_doc_id(workspace_id);
    let sv = workspace_crdt.encode_state_vector();
    let step1 = SyncMessage::SyncStep1(sv).encode();
    let framed = frame_message_v2(&ws_doc_id, &step1);
    ws.send(Message::Binary(framed.into())).await?;

    // Wait for Files-Ready handshake (if server requires it).
    // Stash any binary message received during handshake to process later.
    let mut stashed_binary: Option<Vec<u8>> = None;
    let hs_timeout = tokio::time::Duration::from_secs(10);
    let hs_deadline = tokio::time::Instant::now() + hs_timeout;

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ctrl_msg) = serde_json::from_str::<ControlMessage>(&text) {
                            match ctrl_msg {
                                ControlMessage::FileManifest { .. } => {
                                    let files_ready = r#"{"type":"FilesReady"}"#;
                                    ws.send(Message::Text(files_ready.into())).await?;
                                }
                                ControlMessage::CrdtState { state } => {
                                    if let Ok(state_bytes) = base64::engine::general_purpose::STANDARD.decode(&state) {
                                        let _ = sync_manager.handle_crdt_state(&state_bytes).await;
                                    }
                                    break; // Handshake complete
                                }
                                _ => {} // Ignore other text messages during handshake
                            }
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        // No handshake — server returned Continue directly.
                        stashed_binary = Some(data.to_vec());
                        break;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        ws.close(None).await?;
                        return Ok(0);
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(hs_deadline) => {
                break;
            }
        }
    }

    // Get list of files to sync
    let files: Vec<String> = workspace_crdt
        .list_files()
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let file_count = files.len();

    // Send body SyncStep1 for all files
    for file_path in &files {
        let body_doc_id = format_body_doc_id(workspace_id, file_path);
        let sv = body_manager
            .get_sync_state(file_path)
            .unwrap_or_else(Vec::new);
        let step1 = SyncMessage::SyncStep1(sv).encode();
        let framed = frame_message_v2(&body_doc_id, &step1);
        ws.send(Message::Binary(framed.into())).await?;
    }

    let mut push_count = 0;
    let mut pull_count = 0;
    let mut ws_sent_step2 = false;
    let mut ws_received_step2 = false;
    let mut body_files_sent_step2: HashSet<String> = HashSet::new();
    let mut body_files_received_step2: HashSet<String> = HashSet::new();

    // Timeout based on file count
    let timeout_secs = (10 + file_count / 100).min(60) as u64;
    let timeout = tokio::time::Duration::from_secs(timeout_secs);
    let deadline = tokio::time::Instant::now() + timeout;

    // Collect all binary messages to process (stashed + incoming)
    let mut pending_binaries: Vec<Vec<u8>> = Vec::new();
    if let Some(data) = stashed_binary.take() {
        pending_binaries.push(data);
    }

    loop {
        // Drain pending_binaries first, then wait for new messages
        let data = if let Some(data) = pending_binaries.pop() {
            data
        } else {
            // Wait for next message from WebSocket
            let msg = tokio::select! {
                biased;
                msg = ws.next() => msg,
                _ = tokio::time::sleep_until(deadline) => break,
            };
            match msg {
                Some(Ok(Message::Binary(data))) => data.to_vec(),
                Some(Ok(Message::Text(_))) => continue, // Ignore text in main loop
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(e)) => return Err(e.into()),
                _ => continue,
            }
        };

        {
            let data = &data;
            if let Some((doc_id, payload)) = unframe_message_v2(&data) {
                match parse_doc_id(&doc_id) {
                    Some(DocIdKind::Workspace(_)) => {
                        let messages = SyncMessage::decode_all(&payload)?;
                        for sync_msg in messages {
                            match sync_msg {
                                SyncMessage::SyncStep1(remote_sv) => {
                                    let diff = workspace_crdt.encode_diff(&remote_sv)?;
                                    if diff.len() > 2 {
                                        push_count = 1;
                                    }
                                    let step2 = SyncMessage::SyncStep2(diff).encode();
                                    let framed = frame_message_v2(&doc_id, &step2);
                                    ws.send(Message::Binary(framed.into())).await?;
                                    ws_sent_step2 = true;
                                }
                                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                    ws_received_step2 = true;
                                    if update.len() > 2 {
                                        let (_, changed_files, _) = workspace_crdt
                                            .apply_update_tracking_changes(
                                                &update,
                                                diaryx_core::crdt::UpdateOrigin::Sync,
                                            )?;
                                        pull_count += changed_files.len();
                                    }
                                }
                            }
                        }
                    }
                    Some(DocIdKind::Body { file_path, .. }) => {
                        let messages = SyncMessage::decode_all(&payload)?;
                        for sync_msg in messages {
                            match sync_msg {
                                SyncMessage::SyncStep1(remote_sv) => {
                                    let diff = body_manager.get_diff(&file_path, &remote_sv)?;
                                    if diff.len() > 2 {
                                        push_count += 1;
                                    }
                                    let step2 = SyncMessage::SyncStep2(diff).encode();
                                    let framed = frame_message_v2(&doc_id, &step2);
                                    ws.send(Message::Binary(framed.into())).await?;
                                    body_files_sent_step2.insert(file_path.clone());
                                }
                                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                    body_files_received_step2.insert(file_path.clone());
                                    if update.len() > 2 {
                                        let body_doc = body_manager.get_or_create(&file_path);
                                        body_doc.apply_update(
                                            &update,
                                            diaryx_core::crdt::UpdateOrigin::Sync,
                                        )?;
                                        pull_count += 1;
                                    }
                                }
                            }
                        }
                    }
                    None => {}
                }
            }

            // Check if sync is complete
            let ws_complete = ws_sent_step2 && ws_received_step2;
            let body_complete = body_files_sent_step2.len() >= file_count
                && body_files_received_step2.len() >= file_count;
            if ws_complete && (file_count == 0 || body_complete) {
                break;
            }
        }
    }

    ws.close(None).await?;
    Ok(if pull { pull_count } else { push_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::crdt::{
        BodyDocManager, CrdtStorage, FileMetadata, MemoryStorage, WorkspaceCrdt,
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Helper to create a test workspace CRDT with in-memory storage.
    fn create_test_workspace() -> WorkspaceCrdt {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        WorkspaceCrdt::new(storage)
    }

    /// Helper to create test components (workspace crdt and body manager).
    fn create_test_components() -> (Arc<WorkspaceCrdt>, Arc<BodyDocManager>) {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_manager = Arc::new(BodyDocManager::new(storage));
        (workspace_crdt, body_manager)
    }

    // =========================================================================
    // Empty Update Detection Tests (Critical - catches the 2-byte bug)
    // =========================================================================

    #[test]
    fn test_empty_update_is_two_bytes() {
        // This test validates the Y.js empty update detection logic used throughout
        // the sync code. An empty Y.js update is exactly 2 bytes (header only).
        let workspace = create_test_workspace();

        // Get state vector and compute diff against itself (should be empty)
        let sv = workspace.encode_state_vector();
        let empty_diff = workspace.encode_diff(&sv).unwrap();

        // Y.js empty update is 2 bytes: [0, 0] (no structs, no delete set)
        assert_eq!(
            empty_diff.len(),
            2,
            "Empty Y.js update should be exactly 2 bytes, got {}",
            empty_diff.len()
        );
    }

    #[test]
    fn test_non_empty_update_exceeds_two_bytes() {
        let workspace = create_test_workspace();

        // Add a file to create actual content
        workspace
            .set_file("test.md", FileMetadata::new(Some("Test".to_string())))
            .unwrap();

        // Get state from an empty peer
        let empty_workspace = create_test_workspace();
        let empty_sv = empty_workspace.encode_state_vector();

        // Diff should be non-empty (> 2 bytes)
        let diff = workspace.encode_diff(&empty_sv).unwrap();
        assert!(
            diff.len() > 2,
            "Non-empty Y.js update should exceed 2 bytes, got {} bytes",
            diff.len()
        );
    }

    #[test]
    fn test_body_doc_empty_update_is_two_bytes() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let body_manager = BodyDocManager::new(storage);

        // Create a body doc but don't add content
        let doc = body_manager.get_or_create("test.md");
        let sv = doc.encode_state_vector();
        let diff = doc.encode_diff(&sv).unwrap();

        assert_eq!(
            diff.len(),
            2,
            "Empty body doc diff should be exactly 2 bytes, got {}",
            diff.len()
        );
    }

    #[test]
    fn test_body_doc_non_empty_update_exceeds_two_bytes() {
        let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let body_manager = BodyDocManager::new(storage);

        // Create a body doc and add content
        let doc = body_manager.get_or_create("test.md");
        let _ = doc.set_body("# Hello World\n\nThis is test content.");

        // Get diff from an empty state vector
        let empty_doc_manager = BodyDocManager::new(Arc::new(MemoryStorage::new()));
        let empty_doc = empty_doc_manager.get_or_create("test.md");
        let empty_sv = empty_doc.encode_state_vector();

        let diff = doc.encode_diff(&empty_sv).unwrap();
        assert!(
            diff.len() > 2,
            "Non-empty body doc diff should exceed 2 bytes, got {} bytes",
            diff.len()
        );
    }

    // =========================================================================
    // import_existing_files Tests
    // =========================================================================

    #[test]
    fn test_import_existing_files_counts_correctly() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        // Create some test markdown files
        std::fs::write(workspace_root.join("file1.md"), "# File 1\n\nContent.").unwrap();
        std::fs::write(workspace_root.join("file2.md"), "# File 2\n\nMore content.").unwrap();
        std::fs::write(workspace_root.join("file3.md"), "# File 3\n\nEven more.").unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        assert_eq!(count, 3, "Should import exactly 3 files");
        assert_eq!(workspace_crdt.file_count(), 3);
    }

    #[test]
    fn test_import_skips_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        // Create visible and hidden files
        std::fs::write(workspace_root.join("visible.md"), "# Visible").unwrap();
        std::fs::write(workspace_root.join(".hidden.md"), "# Hidden").unwrap();

        // Create .diaryx directory (should be skipped)
        std::fs::create_dir(workspace_root.join(".diaryx")).unwrap();
        std::fs::write(workspace_root.join(".diaryx").join("crdt.db"), "fake db").unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        assert_eq!(count, 1, "Should only import visible file");
        assert!(workspace_crdt.get_file("visible.md").is_some());
        assert!(workspace_crdt.get_file(".hidden.md").is_none());
    }

    #[test]
    fn test_import_skips_already_tracked_files() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        // Create test files
        std::fs::write(workspace_root.join("existing.md"), "# Existing").unwrap();
        std::fs::write(workspace_root.join("new.md"), "# New").unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        // Pre-add one file to the CRDT
        workspace_crdt
            .set_file(
                "existing.md",
                FileMetadata::new(Some("Already Tracked".to_string())),
            )
            .unwrap();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        // Should only import the new file (existing one is skipped)
        assert_eq!(count, 1, "Should only import 1 new file");
        assert_eq!(workspace_crdt.file_count(), 2);

        // Verify the existing file's metadata wasn't overwritten
        let existing = workspace_crdt.get_file("existing.md").unwrap();
        assert_eq!(existing.title, Some("Already Tracked".to_string()));
    }

    #[test]
    fn test_import_extracts_metadata_from_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        // Create a file with frontmatter
        let content = r#"---
title: My Title
part_of: parent_doc
description: A test file
---

# My Title

Content here."#;
        std::fs::write(workspace_root.join("with_frontmatter.md"), content).unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        assert_eq!(count, 1);

        let metadata = workspace_crdt.get_file("with_frontmatter.md").unwrap();
        assert_eq!(metadata.title, Some("My Title".to_string()));
        assert_eq!(metadata.part_of, Some("parent_doc".to_string()));
        assert_eq!(metadata.description, Some("A test file".to_string()));
    }

    #[test]
    fn test_import_handles_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        // Create nested directory structure
        std::fs::create_dir_all(workspace_root.join("subdir/deep")).unwrap();
        std::fs::write(workspace_root.join("root.md"), "# Root").unwrap();
        std::fs::write(workspace_root.join("subdir/nested.md"), "# Nested").unwrap();
        std::fs::write(workspace_root.join("subdir/deep/deep.md"), "# Deep").unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        assert_eq!(count, 3, "Should import all 3 files from nested structure");
        assert!(workspace_crdt.get_file("root.md").is_some());
        assert!(workspace_crdt.get_file("subdir/nested.md").is_some());
        assert!(workspace_crdt.get_file("subdir/deep/deep.md").is_some());
    }

    #[test]
    fn test_import_skips_non_markdown_files() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        std::fs::write(workspace_root.join("document.md"), "# Doc").unwrap();
        std::fs::write(workspace_root.join("image.png"), "fake png").unwrap();
        std::fs::write(workspace_root.join("notes.txt"), "text file").unwrap();
        std::fs::write(workspace_root.join("data.json"), "{}").unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        let count = import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        assert_eq!(count, 1, "Should only import .md file");
        assert!(workspace_crdt.get_file("document.md").is_some());
    }

    #[test]
    fn test_import_body_content_is_set() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_root = temp_dir.path();

        let content = "# Test Title\n\nThis is the body content.";
        std::fs::write(workspace_root.join("test.md"), content).unwrap();

        let (workspace_crdt, body_manager) = create_test_components();

        import_existing_files(workspace_root, &workspace_crdt, &body_manager);

        // Check that body content was set
        let body_doc = body_manager.get("test.md");
        assert!(body_doc.is_some(), "Body doc should exist");
        let body = body_doc.unwrap().get_body();
        assert_eq!(body, "# Test Title\n\nThis is the body content.");
    }

    // =========================================================================
    // ControlMessage JSON Parsing Tests
    // =========================================================================

    #[test]
    fn test_control_message_sync_progress() {
        let json = r#"{"type": "sync_progress", "completed": 5, "total": 10}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        match msg {
            ControlMessage::SyncProgress { completed, total } => {
                assert_eq!(completed, 5);
                assert_eq!(total, 10);
            }
            _ => panic!("Expected SyncProgress variant"),
        }
    }

    #[test]
    fn test_control_message_sync_complete() {
        let json = r#"{"type": "sync_complete", "files_synced": 42}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        match msg {
            ControlMessage::SyncComplete { files_synced } => {
                assert_eq!(files_synced, 42);
            }
            _ => panic!("Expected SyncComplete variant"),
        }
    }

    #[test]
    fn test_control_message_peer_joined() {
        let json = r#"{"type": "peer_joined", "peer_count": 3}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        match msg {
            ControlMessage::PeerJoined { peer_count } => {
                assert_eq!(peer_count, 3);
            }
            _ => panic!("Expected PeerJoined variant"),
        }
    }

    #[test]
    fn test_control_message_peer_joined_with_default() {
        // Test that missing peer_count defaults to 0
        let json = r#"{"type": "peer_joined"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        match msg {
            ControlMessage::PeerJoined { peer_count } => {
                assert_eq!(peer_count, 0, "Missing peer_count should default to 0");
            }
            _ => panic!("Expected PeerJoined variant"),
        }
    }

    #[test]
    fn test_control_message_peer_left() {
        let json = r#"{"type": "peer_left", "peer_count": 1}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        match msg {
            ControlMessage::PeerLeft { peer_count } => {
                assert_eq!(peer_count, 1);
            }
            _ => panic!("Expected PeerLeft variant"),
        }
    }

    #[test]
    fn test_control_message_unknown_type_is_other() {
        let json = r#"{"type": "unknown_future_message", "data": "some value"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();

        assert!(
            matches!(msg, ControlMessage::Other),
            "Unknown message types should parse as Other"
        );
    }

    #[test]
    fn test_control_message_invalid_json_fails() {
        let json = r#"not valid json"#;
        let result: Result<ControlMessage, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Invalid JSON should fail to parse");
    }

    // =========================================================================
    // Sync Protocol Logic Tests
    // =========================================================================

    #[test]
    fn test_bidirectional_sync_state_vectors() {
        let crdt1 = create_test_workspace();
        let crdt2 = create_test_workspace();

        // Add different files to each
        crdt1
            .set_file("file1.md", FileMetadata::new(Some("File 1".to_string())))
            .unwrap();
        crdt2
            .set_file("file2.md", FileMetadata::new(Some("File 2".to_string())))
            .unwrap();

        // Simulate sync protocol
        let sv1 = crdt1.encode_state_vector();
        let sv2 = crdt2.encode_state_vector();

        let diff1_to_2 = crdt1.encode_diff(&sv2).unwrap();
        let diff2_to_1 = crdt2.encode_diff(&sv1).unwrap();

        // Both diffs should be non-empty (> 2 bytes)
        assert!(diff1_to_2.len() > 2, "Diff from 1 to 2 should have content");
        assert!(diff2_to_1.len() > 2, "Diff from 2 to 1 should have content");

        // Apply diffs
        crdt1
            .apply_update(&diff2_to_1, diaryx_core::crdt::UpdateOrigin::Sync)
            .unwrap();
        crdt2
            .apply_update(&diff1_to_2, diaryx_core::crdt::UpdateOrigin::Sync)
            .unwrap();

        // Both should now have both files
        assert_eq!(crdt1.file_count(), 2);
        assert_eq!(crdt2.file_count(), 2);
        assert!(crdt1.get_file("file1.md").is_some());
        assert!(crdt1.get_file("file2.md").is_some());
        assert!(crdt2.get_file("file1.md").is_some());
        assert!(crdt2.get_file("file2.md").is_some());
    }

    #[test]
    fn test_diff_calculation_already_synced() {
        let crdt1 = create_test_workspace();
        let crdt2 = create_test_workspace();

        // Add same file to crdt1
        crdt1
            .set_file("file.md", FileMetadata::new(Some("File".to_string())))
            .unwrap();

        // Sync crdt1 -> crdt2
        let update = crdt1.encode_state_as_update();
        crdt2
            .apply_update(&update, diaryx_core::crdt::UpdateOrigin::Sync)
            .unwrap();

        // Now both are synced - diff should be empty (2 bytes)
        let sv1 = crdt1.encode_state_vector();
        let sv2 = crdt2.encode_state_vector();

        let diff1_to_2 = crdt1.encode_diff(&sv2).unwrap();
        let diff2_to_1 = crdt2.encode_diff(&sv1).unwrap();

        assert_eq!(diff1_to_2.len(), 2, "Already-synced diff should be 2 bytes");
        assert_eq!(diff2_to_1.len(), 2, "Already-synced diff should be 2 bytes");
    }

    #[test]
    fn test_sync_message_encode_decode() {
        use diaryx_core::crdt::SyncMessage;

        let workspace = create_test_workspace();
        let sv = workspace.encode_state_vector();

        // Test SyncStep1 encoding/decoding
        let step1 = SyncMessage::SyncStep1(sv.clone());
        let encoded = step1.encode();
        let decoded = SyncMessage::decode_all(&encoded).unwrap();

        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            SyncMessage::SyncStep1(decoded_sv) => {
                assert_eq!(decoded_sv, &sv);
            }
            _ => panic!("Expected SyncStep1"),
        }
    }

    #[test]
    fn test_sync_with_body_documents() {
        let storage1: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());
        let storage2: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

        let body_manager1 = BodyDocManager::new(Arc::clone(&storage1));
        let body_manager2 = BodyDocManager::new(Arc::clone(&storage2));

        // Create doc with content in manager1
        let doc1 = body_manager1.get_or_create("test.md");
        let _ = doc1.set_body("Hello, World!");

        // Create an empty doc in manager2 to get a valid state vector
        let doc2 = body_manager2.get_or_create("test.md");
        let sv2 = doc2.encode_state_vector();

        // Get diff from manager1 using the proper state vector
        let diff = body_manager1.get_diff("test.md", &sv2).unwrap();

        // Diff should be non-empty
        assert!(diff.len() > 2, "Diff with content should exceed 2 bytes");

        // Apply diff to doc2
        doc2.apply_update(&diff, diaryx_core::crdt::UpdateOrigin::Sync)
            .unwrap();

        // Content should match
        assert_eq!(doc2.get_body(), "Hello, World!");
    }

    // =========================================================================
    // WebSocket URL Construction Tests
    // =========================================================================

    #[test]
    fn test_ws_url_from_https() {
        let server_url = "https://sync.diaryx.org";
        let ws_server = server_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        assert_eq!(ws_server, "wss://sync.diaryx.org");
    }

    #[test]
    fn test_ws_url_from_http() {
        let server_url = "http://localhost:8080";
        let ws_server = server_url
            .replace("https://", "wss://")
            .replace("http://", "ws://");
        assert_eq!(ws_server, "ws://localhost:8080");
    }
}
