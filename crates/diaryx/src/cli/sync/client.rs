//! Sync client command handlers.
//!
//! Handles start, push, and pull commands using WebSocket connections.
//! The actual sync protocol is handled by `diaryx_core::crdt::SyncClient`.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_core::config::Config;
use diaryx_core::crdt::{
    BodyDocManager, ReconnectConfig, RustSyncManager, SyncClient, SyncClientConfig, SyncEvent,
    SyncEventHandler, SyncHandler, SyncStatus, WorkspaceCrdt,
};
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};

use super::CrdtContext;
use super::progress;
use crate::cli::util::{
    canonicalize_frontmatter_reference, detect_workspace_link_format, parse_link_format,
};

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// CLI event handler that prints sync events to the terminal.
struct CliEventHandler;

impl SyncEventHandler for CliEventHandler {
    fn on_event(&self, event: SyncEvent) {
        match event {
            SyncEvent::StatusChanged { status } => match status {
                SyncStatus::Connecting => {
                    println!("Connecting to sync server...");
                    progress::show_progress(10);
                }
                SyncStatus::Connected => {
                    println!("Connected to sync server");
                    progress::show_progress(30);
                }
                SyncStatus::Syncing => {
                    progress::show_progress(50);
                    println!();
                    println!("Sync is running. Press Ctrl+C to stop.");
                    println!();
                }
                SyncStatus::Synced => {
                    println!("\r\x1b[K  Sync complete");
                    progress::show_progress(100);
                    println!("Watching for changes...");
                    progress::show_indeterminate();
                }
                SyncStatus::Reconnecting { attempt } => {
                    println!("\r\x1b[K  Reconnecting (attempt {})...", attempt);
                }
                SyncStatus::Disconnected => {
                    progress::hide();
                }
            },
            SyncEvent::Progress { completed, total } => {
                if total > 0 {
                    let percent = ((completed as f64 / total as f64) * 100.0) as u8;
                    let scaled = 50 + (percent / 2);
                    progress::show_progress(scaled);
                    print!(
                        "\r\x1b[K  Progress: {}/{} files ({}%)",
                        completed, total, percent
                    );
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }
            SyncEvent::FilesChanged { files } => {
                for file in &files {
                    println!("  Synced: {}", file);
                }
            }
            SyncEvent::BodyChanged { file_path } => {
                println!("\r\x1b[K  Body synced: {}", file_path);
            }
            SyncEvent::Error { message } => {
                eprintln!("  Error: {}", message);
            }
        }
    }
}

/// Scan the workspace and import existing files into the CRDT.
///
/// This is needed for first-time sync when local files exist but the CRDT is empty.
fn import_existing_files(
    workspace_root: &Path,
    workspace_crdt: &WorkspaceCrdt,
    body_manager: &BodyDocManager,
) -> usize {
    use diaryx_core::crdt::{BinaryRef, FileMetadata};
    use std::fs;

    let mut imported = 0;
    let workspace_link_format_hint = detect_workspace_link_format(workspace_root);

    // Walk the workspace directory
    fn walk_dir(
        dir: &Path,
        workspace_root: &Path,
        workspace_crdt: &WorkspaceCrdt,
        body_manager: &BodyDocManager,
        workspace_link_format_hint: Option<diaryx_core::link_parser::LinkFormat>,
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
                    workspace_link_format_hint,
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
                let file_path_for_links = Path::new(&rel_path);
                let file_link_format_hint = fm
                    .get("link_format")
                    .and_then(|v| v.as_str())
                    .and_then(parse_link_format)
                    .or(workspace_link_format_hint);

                let metadata = FileMetadata {
                    filename,
                    title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
                    part_of: fm.get("part_of").and_then(|v| v.as_str()).map(|raw| {
                        canonicalize_frontmatter_reference(
                            raw,
                            file_path_for_links,
                            file_link_format_hint,
                        )
                    }),
                    contents: fm.get("contents").and_then(|v| {
                        v.as_sequence().map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str())
                                .map(|raw| {
                                    canonicalize_frontmatter_reference(
                                        raw,
                                        file_path_for_links,
                                        file_link_format_hint,
                                    )
                                })
                                .collect()
                        })
                    }),
                    attachments: fm
                        .get("attachments")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str())
                                .map(|raw| BinaryRef {
                                    path: canonicalize_frontmatter_reference(
                                        raw,
                                        file_path_for_links,
                                        file_link_format_hint,
                                    ),
                                    source: "local".to_string(),
                                    hash: String::new(),
                                    mime_type: String::new(),
                                    size: 0,
                                    uploaded_at: None,
                                    deleted: false,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
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
        workspace_link_format_hint,
        &mut imported,
    );

    if imported > 0 {
        println!("\r\x1b[K  Imported {} local files into CRDT", imported);
    }

    imported
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

    let workspace_id = config
        .sync_workspace_id
        .as_deref()
        .unwrap_or_else(|| "default");

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

    let client_config = SyncClientConfig {
        server_url: server_url.to_string(),
        workspace_id: workspace_id.to_string(),
        auth_token: Some(session_token.clone()),
        reconnect: ReconnectConfig {
            enabled: false, // CLI doesn't auto-reconnect
            max_attempts: 1,
            ..Default::default()
        },
    };

    let client = SyncClient::new(client_config, sync_manager, Arc::new(CliEventHandler));

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

        client.run_persistent(running).await;
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

    let client_config = SyncClientConfig {
        server_url: server_url.to_string(),
        workspace_id: workspace_id.to_string(),
        auth_token: Some(session_token.clone()),
        reconnect: ReconnectConfig {
            enabled: false,
            max_attempts: 1,
            ..Default::default()
        },
    };

    let client = SyncClient::new(client_config, sync_manager, Arc::new(CliEventHandler));

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match client.run_one_shot().await {
            Ok(stats) => {
                let count = stats.pushed;
                if count == 0 {
                    println!("  Already up to date");
                } else {
                    println!("  Pushed {} items", count);
                }
            }
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

    let client_config = SyncClientConfig {
        server_url: server_url.to_string(),
        workspace_id: workspace_id.to_string(),
        auth_token: Some(session_token.clone()),
        reconnect: ReconnectConfig {
            enabled: false,
            max_attempts: 1,
            ..Default::default()
        },
    };

    let client = SyncClient::new(
        client_config,
        Arc::clone(&sync_manager),
        Arc::new(CliEventHandler),
    );

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match client.run_one_shot().await {
            Ok(stats) => {
                let count = stats.pulled;
                if count == 0 {
                    println!("  Already up to date");
                } else {
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
            }
            Err(e) => eprintln!("  Failed to pull: {}", e),
        }
    });

    println!("Pull complete.");
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
    // Sync Protocol Logic Tests
    // (ControlMessage tests are in diaryx_core::crdt::control_message)
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
