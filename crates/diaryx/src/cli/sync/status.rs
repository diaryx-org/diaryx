//! Status and config command handlers for sync.
//!
//! Handles status display and configuration management.

use std::path::Path;

use diaryx_core::auth::{AuthCredentials, DEFAULT_SYNC_SERVER, NativeFileAuthStorage};
use diaryx_core::config::Config;

use crate::cli::plugin_loader::{CliSyncContext, resolve_sync_runtime_state};

fn current_auth_credentials() -> Option<AuthCredentials> {
    NativeFileAuthStorage::load_global_credentials()
}

fn default_auth_credentials() -> AuthCredentials {
    AuthCredentials {
        server_url: DEFAULT_SYNC_SERVER.to_string(),
        session_token: None,
        email: None,
        workspace_id: None,
    }
}

/// Handle the status command - show sync status.
pub fn handle_status(config: &Config, workspace_root: &Path) {
    let runtime = resolve_sync_runtime_state(config, workspace_root);
    let credentials = current_auth_credentials();

    println!("Sync Status");
    println!("===========");
    println!();

    // Server configuration
    println!("Server: {}", runtime.server_url);

    // Account status
    if let Some(email) = credentials
        .as_ref()
        .and_then(|creds| creds.email.as_deref())
    {
        if runtime.auth_token.is_some() {
            println!("Account: {} (logged in)", email);
        } else {
            println!("Account: {} (not logged in)", email);
        }
    } else {
        println!("Account: (not configured)");
    }

    // Workspace ID
    if let Some(workspace_id) = &runtime.remote_workspace_id {
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

        // Try to get file count from sync plugin
        if let Some(ctx) = CliSyncContext::load(workspace_root) {
            if let Ok(data) = ctx.cmd("ListFiles", serde_json::json!({})) {
                if let Some(files) = data.get("files").and_then(|v| v.as_array()) {
                    println!("  Files tracked: {}", files.len());
                }
            }
        }
    } else {
        println!("CRDT database: (not initialized)");
    }

    // Quick check if we can sync
    println!();
    if runtime.auth_token.is_none() {
        println!("To start syncing, first log in:");
        println!("  diaryx sync login <your-email>");
    } else if runtime.remote_workspace_id.is_none() {
        println!("Workspace ID not configured. It will be set automatically when syncing,");
        println!("or you can set it manually:");
        println!("  diaryx sync config --workspace-id <id>");
    } else {
        println!("Ready to sync! Start with:");
        println!("  diaryx sync start");
    }
}

/// Handle the config command - configure sync settings.
pub fn handle_config(server: Option<String>, workspace_id: Option<String>, show: bool) {
    let credentials = current_auth_credentials().unwrap_or_else(default_auth_credentials);

    // If --show or no options, display current config
    if show || (server.is_none() && workspace_id.is_none()) {
        println!("Sync Configuration");
        println!("==================");
        println!();
        println!("Server URL: {}", credentials.server_url);
        println!(
            "Email: {}",
            credentials.email.as_deref().unwrap_or("(not set)")
        );
        println!(
            "Session: {}",
            if credentials.session_token.is_some() {
                "active"
            } else {
                "(not logged in)"
            }
        );
        println!(
            "Workspace ID: {}",
            credentials.workspace_id.as_deref().unwrap_or("(not set)")
        );
        return;
    }

    // Update configuration
    let mut updated_credentials = credentials;
    let mut changes = Vec::new();

    if let Some(s) = server {
        updated_credentials.server_url = s.clone();
        changes.push(format!("Server URL: {}", s));
    }

    if let Some(wid) = workspace_id {
        updated_credentials.workspace_id = Some(wid.clone());
        changes.push(format!("Workspace ID: {}", wid));
    }

    if changes.is_empty() {
        println!("No changes made.");
        return;
    }

    // Save config
    match NativeFileAuthStorage::save_global_credentials(&updated_credentials) {
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

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Config State Tests
    // =========================================================================

    #[test]
    fn test_default_auth_credentials() {
        let credentials = default_auth_credentials();

        assert_eq!(credentials.server_url, DEFAULT_SYNC_SERVER);
        assert!(credentials.email.is_none());
        assert!(credentials.session_token.is_none());
        assert!(credentials.workspace_id.is_none());
    }

    #[test]
    fn test_status_session_active_display() {
        let credentials = AuthCredentials {
            server_url: DEFAULT_SYNC_SERVER.to_string(),
            session_token: Some("token".to_string()),
            email: None,
            workspace_id: None,
        };

        let session_display = if credentials.session_token.is_some() {
            "active"
        } else {
            "(not logged in)"
        };

        assert_eq!(session_display, "active");
    }

    #[test]
    fn test_status_session_inactive_display() {
        let credentials = default_auth_credentials();

        let session_display = if credentials.session_token.is_some() {
            "active"
        } else {
            "(not logged in)"
        };

        assert_eq!(session_display, "(not logged in)");
    }

    #[test]
    fn test_status_optional_field_display() {
        let credentials = default_auth_credentials();

        let email_display = credentials.email.as_deref().unwrap_or("(not set)");
        let workspace_display = credentials.workspace_id.as_deref().unwrap_or("(not set)");

        assert_eq!(email_display, "(not set)");
        assert_eq!(workspace_display, "(not set)");
    }

    // =========================================================================
    // Config Update Logic Tests
    // =========================================================================

    #[test]
    fn test_config_update_server_url() {
        let mut credentials = default_auth_credentials();
        credentials.server_url = "https://new.server.com".to_string();

        assert_eq!(credentials.server_url, "https://new.server.com");
    }

    #[test]
    fn test_config_update_workspace_id() {
        let mut credentials = default_auth_credentials();
        credentials.workspace_id = Some("new-workspace-id".to_string());

        assert_eq!(
            credentials.workspace_id.as_deref(),
            Some("new-workspace-id")
        );
    }

    #[test]
    fn test_config_update_both_fields() {
        let mut credentials = default_auth_credentials();
        credentials.server_url = "https://server.com".to_string();
        credentials.workspace_id = Some("workspace-id".to_string());

        assert_eq!(credentials.server_url, "https://server.com");
        assert_eq!(credentials.workspace_id.as_deref(), Some("workspace-id"));
    }

    #[test]
    fn test_config_no_changes_when_none() {
        // Simulate handle_config with no options
        let server: Option<String> = None;
        let workspace_id: Option<String> = None;

        let mut changes = Vec::new();
        if let Some(s) = server {
            changes.push(format!("Server URL: {}", s));
        }
        if let Some(wid) = workspace_id {
            changes.push(format!("Workspace ID: {}", wid));
        }

        assert!(changes.is_empty(), "No changes should be recorded");
    }

    #[test]
    fn test_status_with_configured_fields() {
        let credentials = AuthCredentials {
            server_url: "https://sync.example.com".to_string(),
            email: Some("user@example.com".to_string()),
            session_token: None,
            workspace_id: Some("ws-123".to_string()),
        };

        let server_display = credentials.server_url.as_str();
        let email_display = credentials.email.as_deref().unwrap_or("(not set)");
        let workspace_display = credentials.workspace_id.as_deref().unwrap_or("(not set)");

        assert_eq!(server_display, "https://sync.example.com");
        assert_eq!(email_display, "user@example.com");
        assert_eq!(workspace_display, "ws-123");
    }
}
