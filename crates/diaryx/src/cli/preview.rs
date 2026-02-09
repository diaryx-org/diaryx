//! CLI handler for preview command — live preview of published workspace

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use axum::Router;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::publish::{PublishOptions, Publisher};
use tower_http::services::ServeDir;

/// RAII guard that removes a temp directory on drop.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Handle the preview command
pub fn handle_preview(
    workspace_override: Option<PathBuf>,
    port: u16,
    no_open: bool,
    audience: Option<String>,
    title: Option<String>,
) {
    let workspace_root = match super::publish::resolve_workspace_for_publish(workspace_override) {
        Ok(root) => root,
        Err(e) => {
            eprintln!("Error: {}", e);
            return;
        }
    };

    let temp_dir = std::env::temp_dir().join(format!("diaryx-preview-{}", std::process::id()));
    let _guard = TempDirGuard(temp_dir.clone());

    // Initial publish
    if !do_publish(&workspace_root, &temp_dir, &audience, &title) {
        return;
    }

    let url = format!("http://localhost:{}", port);
    println!("Preview server running at {}", url);
    println!("Press Ctrl+C to stop");

    if !no_open {
        let _ = open::that(&url);
    }

    let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    rt.block_on(async {
        tokio::select! {
            result = serve(&temp_dir, port) => {
                if let Err(e) = result {
                    eprintln!("Server error: {}", e);
                }
            }
            _ = watch_and_rebuild(&workspace_root, &temp_dir, &audience, &title) => {}
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down preview server...");
            }
        }
    });
}

/// Publish workspace to the destination directory.
fn do_publish(
    workspace_root: &Path,
    dest: &Path,
    audience: &Option<String>,
    title: &Option<String>,
) -> bool {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let publisher = Publisher::new(fs);
    let options = PublishOptions {
        single_file: false,
        title: title.clone(),
        audience: audience.clone(),
        force: true,
    };

    match futures_lite::future::block_on(publisher.publish(workspace_root, dest, &options)) {
        Ok(result) => {
            if result.files_processed == 0 {
                eprintln!("No files to publish");
                if audience.is_some() {
                    eprintln!("  (no files match the specified audience)");
                }
                return false;
            }
            true
        }
        Err(e) => {
            eprintln!("Publish failed: {}", e);
            false
        }
    }
}

/// Serve static files from a directory over HTTP.
async fn serve(dir: &Path, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new().fallback_service(ServeDir::new(dir));
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Poll the workspace for file changes and rebuild when detected.
async fn watch_and_rebuild(
    workspace_root: &Path,
    dest: &Path,
    audience: &Option<String>,
    title: &Option<String>,
) {
    // The workspace root is an index file — watch its parent directory
    let watch_dir = workspace_root
        .parent()
        .unwrap_or(workspace_root)
        .to_path_buf();
    let mut last_publish = Instant::now();

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        if has_changes_since(&watch_dir, last_publish) {
            println!("Changes detected, rebuilding...");
            if do_publish(workspace_root, dest, audience, title) {
                println!("Rebuild complete.");
            }
            last_publish = Instant::now();
        }
    }
}

/// Check if any workspace content files have been modified since the given instant.
fn has_changes_since(dir: &Path, since: Instant) -> bool {
    let threshold = SystemTime::now() - since.elapsed();
    walk_for_changes(dir, threshold)
}

/// Recursively walk a directory looking for recently modified content files.
fn walk_for_changes(dir: &Path, threshold: SystemTime) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
        }

        if path.is_dir() {
            if walk_for_changes(&path, threshold) {
                return true;
            }
        } else if is_content_file(&path) {
            if let Ok(meta) = path.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified > threshold {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if a file is a workspace content file worth watching.
fn is_content_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map_or(false, |ext| matches!(ext, "md" | "yaml" | "yml"))
}
