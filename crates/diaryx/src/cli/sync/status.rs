//! Status and config command handlers for sync.
//!
//! Handles status display and configuration management.

use std::path::Path;

use diaryx_core::config::Config;
use diaryx_core::crdt::SqliteStorage;

/// Handle the status command - show sync status.
pub fn handle_status(config: &Config, workspace_root: &Path) {
    println!("Sync Status");
    println!("===========");
    println!();

    // Server configuration
    if let Some(server) = &config.sync_server_url {
        println!("Server: {}", server);
    } else {
        println!("Server: (not configured)");
    }

    // Account status
    if let Some(email) = &config.sync_email {
        if config.sync_session_token.is_some() {
            println!("Account: {} (logged in)", email);
        } else {
            println!("Account: {} (not logged in)", email);
        }
    } else {
        println!("Account: (not configured)");
    }

    // Workspace ID
    if let Some(workspace_id) = &config.sync_workspace_id {
        println!("Workspace ID: {}", workspace_id);
    } else {
        println!("Workspace ID: (not configured)");
    }

    // Local workspace
    println!("Workspace root: {}", workspace_root.display());

    // CRDT database status
    let crdt_db = workspace_root.join(".diaryx").join("crdt.db");
    if crdt_db.exists() {
        println!("CRDT database: {}", crdt_db.display());

        // Try to get some stats from the database
        if let Ok(storage) = SqliteStorage::open(&crdt_db) {
            if let Ok(files) = storage.query_active_files() {
                println!("  Files tracked: {}", files.len());
            }
        }
    } else {
        println!("CRDT database: (not initialized)");
    }

    // Quick check if we can sync
    println!();
    if config.sync_session_token.is_none() {
        println!("To start syncing, first log in:");
        println!("  diaryx sync login <your-email>");
    } else if config.sync_workspace_id.is_none() {
        println!("Workspace ID not configured. It will be set automatically when syncing,");
        println!("or you can set it manually:");
        println!("  diaryx sync config --workspace-id <id>");
    } else {
        println!("Ready to sync! Start with:");
        println!("  diaryx sync start");
    }
}

/// Handle the config command - configure sync settings.
pub fn handle_config(
    config: &Config,
    server: Option<String>,
    workspace_id: Option<String>,
    show: bool,
) {
    // If --show or no options, display current config
    if show || (server.is_none() && workspace_id.is_none()) {
        println!("Sync Configuration");
        println!("==================");
        println!();
        println!(
            "Server URL: {}",
            config.sync_server_url.as_deref().unwrap_or("(not set)")
        );
        println!(
            "Email: {}",
            config.sync_email.as_deref().unwrap_or("(not set)")
        );
        println!(
            "Session: {}",
            if config.sync_session_token.is_some() {
                "active"
            } else {
                "(not logged in)"
            }
        );
        println!(
            "Workspace ID: {}",
            config.sync_workspace_id.as_deref().unwrap_or("(not set)")
        );
        return;
    }

    // Update configuration
    let mut new_config = config.clone();
    let mut changes = Vec::new();

    if let Some(s) = server {
        new_config.sync_server_url = Some(s.clone());
        changes.push(format!("Server URL: {}", s));
    }

    if let Some(wid) = workspace_id {
        new_config.sync_workspace_id = Some(wid.clone());
        changes.push(format!("Workspace ID: {}", wid));
    }

    if changes.is_empty() {
        println!("No changes made.");
        return;
    }

    // Save config
    match new_config.save() {
        Ok(()) => {
            println!("Configuration updated:");
            for change in changes {
                println!("  {}", change);
            }
        }
        Err(e) => {
            eprintln!("Failed to save configuration: {}", e);
        }
    }
}
