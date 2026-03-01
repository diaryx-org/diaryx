//! Sync client command handlers.
//!
//! Uses the Extism sync plugin via `CliSyncContext` for all CRDT operations.
//! The WebSocket bridge (`WsBridge`) handles the transport layer.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_core::config::Config;

use crate::cli::plugin_loader::CliSyncContext;

use super::progress;
use super::ws_bridge::{WsBridge, WsBridgeConfig};

const DEFAULT_SYNC_SERVER: &str = "https://sync.diaryx.org";

/// Handle the start command - start continuous sync via WS bridge.
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

    let workspace_id = config.sync_workspace_id.as_deref().unwrap_or("default");

    println!("Starting sync...");
    println!("  Server: {}", server_url);
    println!("  Workspace: {}", workspace_id);
    println!("  Local path: {}", workspace_root.display());
    println!();

    // Initialize sync plugin context
    let ctx = match CliSyncContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    // Initialize workspace CRDT from disk if empty
    match ctx.cmd(
        "InitializeWorkspaceCrdt",
        serde_json::json!({
            "workspace_path": workspace_root.to_string_lossy(),
        }),
    ) {
        Ok(result) => {
            if let Some(count) = result.get("file_count").and_then(|v| v.as_u64()) {
                if count > 0 {
                    println!("  Initialized {} files", count);
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to initialize workspace: {}", e);
        }
    }

    // Build WS URL
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let ws_url = format!("{}/sync2", ws_server);

    let bridge_config = WsBridgeConfig {
        ws_url,
        auth_token: Some(session_token.clone()),
        workspace_id: workspace_id.to_string(),
    };

    // Take the plugin out of CliSyncContext for the bridge
    let plugin = ctx.into_plugin();
    let bridge = WsBridge::new(bridge_config, plugin);

    // Set up shutdown flag
    let running = Arc::new(AtomicBool::new(true));

    // Ensure progress bar is cleared on exit
    let _progress_guard = progress::ProgressGuard::new();

    // Show connecting state
    progress::show_indeterminate();

    // Run the sync loop
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match bridge.run(running).await {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Sync error: {}", e);
            }
        }
    });

    println!("Sync stopped.");
}

/// Handle the push command - one-shot push via WS bridge.
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

    // Initialize sync plugin and populate from disk
    let ctx = match CliSyncContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    match ctx.cmd(
        "InitializeWorkspaceCrdt",
        serde_json::json!({
            "workspace_path": workspace_root.to_string_lossy(),
        }),
    ) {
        Ok(result) => {
            if let Some(count) = result.get("file_count").and_then(|v| v.as_u64()) {
                println!("  {} files ready to push", count);
            }
        }
        Err(e) => {
            eprintln!("Failed to initialize workspace: {}", e);
            return;
        }
    }

    // Build WS URL and run one-shot sync
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let ws_url = format!("{}/sync2", ws_server);

    let bridge_config = WsBridgeConfig {
        ws_url,
        auth_token: Some(session_token.clone()),
        workspace_id: workspace_id.to_string(),
    };

    let plugin = ctx.into_plugin();
    let bridge = WsBridge::new(bridge_config, plugin);

    // Run sync with auto-stop after completion
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    // Set a timeout for push — stop after 30 seconds
    runtime.block_on(async {
        tokio::select! {
            result = bridge.run(running) => {
                match result {
                    Ok(()) => println!("Push complete."),
                    Err(e) => eprintln!("Push failed: {}", e),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                running_clone.store(false, Ordering::SeqCst);
                println!("Push timed out after 30 seconds.");
            }
        }
    });
}

/// Handle the pull command - one-shot pull via WS bridge.
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

    // Initialize sync plugin
    let ctx = match CliSyncContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    // Build WS URL and run one-shot sync (pull-only)
    let ws_server = server_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let ws_url = format!("{}/sync2", ws_server);

    let bridge_config = WsBridgeConfig {
        ws_url,
        auth_token: Some(session_token.clone()),
        workspace_id: workspace_id.to_string(),
    };

    let plugin = ctx.into_plugin();
    let bridge = WsBridge::new(bridge_config, plugin);

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        tokio::select! {
            result = bridge.run(running) => {
                match result {
                    Ok(()) => println!("Pull complete."),
                    Err(e) => eprintln!("Pull failed: {}", e),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                running_clone.store(false, Ordering::SeqCst);
                println!("Pull timed out after 30 seconds.");
            }
        }
    });
}
