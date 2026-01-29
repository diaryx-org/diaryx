//! Sync client command handlers.
//!
//! Handles start, push, and pull commands using WebSocket connections.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_core::config::Config;
use diaryx_core::crdt::{
    BodyDocManager, CrdtStorage, RustSyncManager, SqliteStorage, SyncHandler, SyncMessage,
    WorkspaceCrdt, frame_body_message, unframe_body_message,
};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

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

    // Initialize CRDT storage
    let crdt_dir = workspace_root.join(".diaryx");
    if !crdt_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&crdt_dir) {
            eprintln!("Failed to create .diaryx directory: {}", e);
            return;
        }
    }

    let crdt_db = crdt_dir.join("crdt.db");
    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    // Create CRDT components
    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let sync_manager = Arc::new(RustSyncManager::new(
        Arc::clone(&workspace_crdt),
        Arc::clone(&body_manager),
        Arc::clone(&sync_handler),
    ));

    // Build WebSocket URLs
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nShutting down sync...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // Run the sync loop
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        run_sync_loop(
            &metadata_url,
            &body_url,
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

    // Initialize CRDT storage
    let crdt_db = workspace_root.join(".diaryx").join("crdt.db");
    if !crdt_db.exists() {
        println!("No local CRDT database found. Nothing to push.");
        return;
    }

    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        // Connect and push metadata
        match do_one_shot_sync(&metadata_url, &workspace_crdt, false).await {
            Ok(count) => println!("  Pushed {} metadata updates", count),
            Err(e) => eprintln!("  Failed to push metadata: {}", e),
        }

        // Connect and push bodies
        match do_one_shot_body_sync(&body_url, &body_manager, false).await {
            Ok(count) => println!("  Pushed {} body updates", count),
            Err(e) => eprintln!("  Failed to push bodies: {}", e),
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

    // Initialize CRDT storage
    let crdt_dir = workspace_root.join(".diaryx");
    if !crdt_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&crdt_dir) {
            eprintln!("Failed to create .diaryx directory: {}", e);
            return;
        }
    }

    let crdt_db = crdt_dir.join("crdt.db");
    let storage: Arc<dyn CrdtStorage> = match SqliteStorage::open(&crdt_db) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("Failed to open CRDT database: {}", e);
            return;
        }
    };

    let workspace_crdt = Arc::new(
        WorkspaceCrdt::load(Arc::clone(&storage))
            .unwrap_or_else(|_| WorkspaceCrdt::new(storage.clone())),
    );
    let body_manager = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let sync_handler = Arc::new(SyncHandler::new(fs));
    sync_handler.set_workspace_root(workspace_root.to_path_buf());

    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");

    let metadata_url = format!(
        "{}/sync?doc={}&token={}",
        ws_server, workspace_id, session_token
    );

    let body_url = format!(
        "{}/sync?doc={}&multiplexed=true&token={}",
        ws_server, workspace_id, session_token
    );

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        // Connect and pull metadata
        match do_one_shot_sync(&metadata_url, &workspace_crdt, true).await {
            Ok(count) => {
                println!("  Received {} metadata updates", count);

                // Write updated files to disk
                // list_files() returns Vec<(String, FileMetadata)>
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
            Err(e) => eprintln!("  Failed to pull metadata: {}", e),
        }

        // Connect and pull bodies
        match do_one_shot_body_sync(&body_url, &body_manager, true).await {
            Ok(count) => println!("  Received {} body updates", count),
            Err(e) => eprintln!("  Failed to pull bodies: {}", e),
        }
    });

    println!("Pull complete.");
}

/// Run the main sync loop with two WebSocket connections.
async fn run_sync_loop(
    metadata_url: &str,
    body_url: &str,
    sync_manager: Arc<RustSyncManager<SyncToAsyncFs<RealFileSystem>>>,
    workspace_crdt: Arc<WorkspaceCrdt>,
    running: Arc<AtomicBool>,
) {
    println!("Connecting to sync server...");

    // Connect to metadata WebSocket
    let metadata_ws = match connect_async(metadata_url).await {
        Ok((ws, _)) => {
            println!("Connected to metadata sync");
            Some(ws)
        }
        Err(e) => {
            eprintln!("Failed to connect to metadata sync: {}", e);
            None
        }
    };

    // Connect to body WebSocket
    let body_ws = match connect_async(body_url).await {
        Ok((ws, _)) => {
            println!("Connected to body sync");
            Some(ws)
        }
        Err(e) => {
            eprintln!("Failed to connect to body sync: {}", e);
            None
        }
    };

    if metadata_ws.is_none() && body_ws.is_none() {
        eprintln!("No connections established. Exiting.");
        return;
    }

    println!();
    println!("Sync is running. Press Ctrl+C to stop.");
    println!();

    // Spawn metadata WebSocket handler
    if let Some(mut ws) = metadata_ws {
        let running_clone = running.clone();
        let sync_manager_clone = Arc::clone(&sync_manager);

        tokio::spawn(async move {
            // Send SyncStep1
            let step1 = sync_manager_clone.create_workspace_sync_step1();
            if let Err(e) = ws.send(Message::Binary(step1.into())).await {
                eprintln!("Failed to send metadata SyncStep1: {}", e);
                return;
            }

            while running_clone.load(Ordering::SeqCst) {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                match sync_manager_clone.handle_workspace_message(&data, true).await {
                                    Ok(result) => {
                                        if let Some(response) = result.response {
                                            if let Err(e) = ws.send(Message::Binary(response.into())).await {
                                                eprintln!("Failed to send metadata response: {}", e);
                                            }
                                        }
                                        if result.sync_complete {
                                            println!("Initial metadata sync complete");
                                        }
                                        if !result.changed_files.is_empty() {
                                            for file in &result.changed_files {
                                                println!("  Updated: {}", file);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Error handling metadata message: {}", e);
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                println!("Metadata connection closed by server");
                                break;
                            }
                            Some(Err(e)) => {
                                eprintln!("Metadata WebSocket error: {}", e);
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
        });
    }

    // Spawn body WebSocket handler
    if let Some(mut ws) = body_ws {
        let running_clone = running.clone();
        let sync_manager_clone = Arc::clone(&sync_manager);
        let workspace_crdt_clone = Arc::clone(&workspace_crdt);

        tokio::spawn(async move {
            // Send SyncStep1 for all known files
            // list_files() returns Vec<(String, FileMetadata)>
            for (file_path, _metadata) in workspace_crdt_clone.list_files() {
                let step1 = sync_manager_clone.create_body_sync_step1(&file_path);
                let framed = frame_body_message(&file_path, &step1);
                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                    eprintln!("Failed to send body SyncStep1 for {}: {}", file_path, e);
                }
            }

            while running_clone.load(Ordering::SeqCst) {
                tokio::select! {
                    msg = ws.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                // Unframe the multiplexed message
                                if let Some((file_path, body_msg)) = unframe_body_message(&data) {
                                    match sync_manager_clone.handle_body_message(&file_path, &body_msg, true).await {
                                        Ok(result) => {
                                            if let Some(response) = result.response {
                                                let framed = frame_body_message(&file_path, &response);
                                                if let Err(e) = ws.send(Message::Binary(framed.into())).await {
                                                    eprintln!("Failed to send body response: {}", e);
                                                }
                                            }
                                            if result.content.is_some() && !result.is_echo {
                                                println!("  Body updated: {}", file_path);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error handling body message for {}: {}", file_path, e);
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                println!("Body connection closed by server");
                                break;
                            }
                            Some(Err(e)) => {
                                eprintln!("Body WebSocket error: {}", e);
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
        });
    }

    // Wait for shutdown signal
    while running.load(Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

/// Perform one-shot metadata sync.
async fn do_one_shot_sync(
    url: &str,
    workspace_crdt: &WorkspaceCrdt,
    pull: bool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(url).await?;

    // Send our state vector
    let sv = workspace_crdt.encode_state_vector();
    let step1 = SyncMessage::SyncStep1(sv).encode();
    ws.send(Message::Binary(step1.into())).await?;

    let mut update_count = 0;

    // Receive and process messages until sync is complete
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let messages = SyncMessage::decode_all(&data)?;
                        for sync_msg in messages {
                            match sync_msg {
                                SyncMessage::SyncStep1(remote_sv) => {
                                    // Send our diff
                                    let diff = workspace_crdt.encode_diff(&remote_sv)?;
                                    let step2 = SyncMessage::SyncStep2(diff).encode();
                                    ws.send(Message::Binary(step2.into())).await?;
                                }
                                SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                    if pull && !update.is_empty() {
                                        workspace_crdt.apply_update(&update, diaryx_core::crdt::UpdateOrigin::Sync)?;
                                        update_count += 1;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    ws.close(None).await?;
    Ok(update_count)
}

/// Perform one-shot body sync.
async fn do_one_shot_body_sync(
    url: &str,
    body_manager: &BodyDocManager,
    pull: bool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let (mut ws, _) = connect_async(url).await?;

    let mut update_count = 0;

    // For pull, we just wait for any incoming updates
    // For push, we'd send our body states (simplified for now)

    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        if let Some((file_path, body_msg)) = unframe_body_message(&data) {
                            if pull {
                                let body_doc = body_manager.get_or_create(&file_path);
                                let messages = SyncMessage::decode_all(&body_msg)?;
                                for sync_msg in messages {
                                    match sync_msg {
                                        SyncMessage::SyncStep2(update) | SyncMessage::Update(update) => {
                                            if !update.is_empty() {
                                                body_doc.apply_update(&update, diaryx_core::crdt::UpdateOrigin::Sync)?;
                                                update_count += 1;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => return Err(e.into()),
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    ws.close(None).await?;
    Ok(update_count)
}
