//! Sync client command handlers.
//!
//! Uses the Extism sync plugin via `CliSyncContext` for all CRDT operations.
//! The generic host `TokioWebSocketBridge` handles the transport layer.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use diaryx_core::config::Config;
use diaryx_extism::WebSocketBridge;

use crate::cli::plugin_loader::{CliSyncContext, resolve_sync_runtime_state};

use super::progress;
const ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(30);
const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(200);

fn connect_sync_bridge(
    ctx: &CliSyncContext,
    server_url: &str,
    workspace_id: &str,
    auth_token: &str,
    write_to_disk: bool,
) -> Result<(), String> {
    let request = serde_json::json!({
        "type": "connect",
        "server_url": server_url,
        "workspace_id": workspace_id,
        "auth_token": auth_token,
        "write_to_disk": write_to_disk,
    });

    ctx.websocket_bridge()
        .request(&request.to_string())
        .map(|_| ())
}

fn disconnect_sync_bridge(ctx: &CliSyncContext) {
    let _ = ctx.websocket_bridge().request(r#"{"type":"disconnect"}"#);
}

fn wait_for_sync_completion(ctx: &CliSyncContext, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let complete = ctx
            .cmd("IsSyncComplete", serde_json::json!({}))?
            .get("complete")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if complete {
            return Ok(true);
        }

        thread::sleep(SYNC_POLL_INTERVAL);
    }

    Ok(false)
}

/// Handle the start command - start continuous sync via WS bridge.
pub fn handle_start(config: &Config, workspace_root: &Path) {
    let runtime = resolve_sync_runtime_state(config, workspace_root);

    // Validate configuration
    let Some(session_token) = runtime.auth_token.as_deref() else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = runtime.server_url.as_str();
    let workspace_id = runtime.remote_workspace_id.as_deref().unwrap_or("default");

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

    // Ensure progress bar is cleared on exit
    let _progress_guard = progress::ProgressGuard::new();

    // Show connecting state
    progress::show_indeterminate();

    if let Err(e) = connect_sync_bridge(&ctx, server_url, workspace_id, session_token, true) {
        eprintln!("Sync error: {}", e);
        return;
    }

    // Run until interrupted; the generic websocket bridge handles reconnects.
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
    runtime.block_on(async {
        let _ = tokio::signal::ctrl_c().await;
    });

    disconnect_sync_bridge(&ctx);

    println!("Sync stopped.");
}

/// Handle the push command - one-shot push via WS bridge.
pub fn handle_push(config: &Config, workspace_root: &Path) {
    let runtime = resolve_sync_runtime_state(config, workspace_root);

    let Some(session_token) = runtime.auth_token.as_deref() else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = runtime.server_url.as_str();
    let workspace_id = runtime.remote_workspace_id.as_deref().unwrap_or("default");

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

    if let Err(e) = connect_sync_bridge(&ctx, server_url, workspace_id, session_token, false) {
        eprintln!("Push failed: {}", e);
        return;
    }

    let result = wait_for_sync_completion(&ctx, ONE_SHOT_TIMEOUT);
    disconnect_sync_bridge(&ctx);

    match result {
        Ok(true) => println!("Push complete."),
        Ok(false) => println!("Push timed out after 30 seconds."),
        Err(e) => eprintln!("Push failed: {}", e),
    }
}

/// Handle the pull command - one-shot pull via WS bridge.
pub fn handle_pull(config: &Config, workspace_root: &Path) {
    let runtime = resolve_sync_runtime_state(config, workspace_root);

    let Some(session_token) = runtime.auth_token.as_deref() else {
        eprintln!("Not logged in. Please log in first:");
        eprintln!("  diaryx sync login <your-email>");
        return;
    };

    let server_url = runtime.server_url.as_str();
    let workspace_id = runtime.remote_workspace_id.as_deref().unwrap_or("default");

    println!("Pulling remote changes...");

    // Initialize sync plugin
    let ctx = match CliSyncContext::load_or_create(workspace_root) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    if let Err(e) = connect_sync_bridge(&ctx, server_url, workspace_id, session_token, true) {
        eprintln!("Pull failed: {}", e);
        return;
    }

    let result = wait_for_sync_completion(&ctx, ONE_SHOT_TIMEOUT);
    disconnect_sync_bridge(&ctx);

    match result {
        Ok(true) => println!("Pull complete."),
        Ok(false) => println!("Pull timed out after 30 seconds."),
        Err(e) => eprintln!("Pull failed: {}", e),
    }
}
