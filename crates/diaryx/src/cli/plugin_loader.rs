//! Plugin loading and context for CLI Extism integration.
//!
//! Provides `CliSyncContext` which replaces the old `CrdtContext` for all
//! sync plugin interactions, routing through Extism plugin commands instead
//! of direct CRDT API calls.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_extism::{EventEmitter, ExtismPluginAdapter, HostContext, load_plugin_from_wasm};
use serde_json::Value as JsonValue;

use super::plugin_storage::CliPluginStorage;

/// CLI event emitter — logs plugin events to stderr.
pub struct CliEventEmitter;

impl EventEmitter for CliEventEmitter {
    fn emit(&self, event_json: &str) {
        // Parse event to extract type for logging
        if let Ok(event) = serde_json::from_str::<JsonValue>(event_json) {
            let event_type = event
                .get("event_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            match event_type {
                "status_changed" => {
                    if let Some(status) = event
                        .get("payload")
                        .and_then(|p| p.get("status"))
                        .and_then(|s| s.as_str())
                    {
                        log::debug!("Sync status: {}", status);
                    }
                }
                "files_changed" => {
                    if let Some(files) = event
                        .get("payload")
                        .and_then(|p| p.get("files"))
                        .and_then(|f| f.as_array())
                    {
                        for file in files {
                            if let Some(path) = file.as_str() {
                                println!("  Synced: {}", path);
                            }
                        }
                    }
                }
                "body_changed" => {
                    if let Some(path) = event
                        .get("payload")
                        .and_then(|p| p.get("file_path"))
                        .and_then(|s| s.as_str())
                    {
                        println!("\r\x1b[K  Body synced: {}", path);
                    }
                }
                "error" => {
                    if let Some(msg) = event
                        .get("payload")
                        .and_then(|p| p.get("message"))
                        .and_then(|s| s.as_str())
                    {
                        eprintln!("  Error: {}", msg);
                    }
                }
                _ => {
                    log::debug!("Plugin event: {}", event_type);
                }
            }
        }
    }
}

/// Sync plugin context for the CLI.
///
/// Wraps an `ExtismPluginAdapter` loaded from `diaryx_sync.wasm`.
/// All CRDT operations go through plugin commands.
pub struct CliSyncContext {
    plugin: ExtismPluginAdapter,
}

impl CliSyncContext {
    /// Try to load the sync plugin for an existing workspace.
    ///
    /// Returns `None` if the CRDT database doesn't exist (user hasn't synced yet).
    pub fn load(workspace_root: &Path) -> Option<Self> {
        let db_path = workspace_root.join(".diaryx/crdt.db");
        if !db_path.exists() {
            return None;
        }
        Self::load_or_create(workspace_root).ok()
    }

    /// Load or create the sync plugin context for a workspace.
    pub fn load_or_create(workspace_root: &Path) -> Result<Self, String> {
        let diaryx_dir = workspace_root.join(".diaryx");
        if !diaryx_dir.exists() {
            std::fs::create_dir_all(&diaryx_dir)
                .map_err(|e| format!("Failed to create .diaryx directory: {}", e))?;
        }

        let plugin = load_sync_plugin(workspace_root)?;
        Ok(Self { plugin })
    }

    /// Send a command to the sync plugin and return the result.
    pub fn cmd(&self, command: &str, params: JsonValue) -> Result<JsonValue, String> {
        let input = serde_json::json!({
            "command": command,
            "params": params,
        });

        let output = self
            .plugin
            .call_guest("handle_command", &input.to_string())
            .map_err(|e| format!("Plugin call failed: {}", e))?;

        let response: JsonValue =
            serde_json::from_str(&output).map_err(|e| format!("Invalid plugin response: {}", e))?;

        if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
            Ok(response.get("data").cloned().unwrap_or(JsonValue::Null))
        } else {
            let error_msg = response
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown plugin error");
            Err(error_msg.to_string())
        }
    }

    /// Consume the context and return the underlying plugin adapter.
    ///
    /// Used when ownership of the plugin needs to be transferred (e.g., to WsBridge).
    pub fn into_plugin(self) -> ExtismPluginAdapter {
        self.plugin
    }
}

/// Publish plugin context for the CLI.
///
/// Wraps an `ExtismPluginAdapter` loaded from `diaryx_publish.wasm`.
pub struct CliPublishContext {
    plugin: ExtismPluginAdapter,
}

impl CliPublishContext {
    /// Load the publish plugin.
    pub fn load(workspace_root: &Path) -> Result<Self, String> {
        let plugin = load_publish_plugin(workspace_root)?;
        Ok(Self { plugin })
    }

    /// Send a command to the publish plugin and return the result.
    pub fn cmd(&self, command: &str, params: JsonValue) -> Result<JsonValue, String> {
        let input = serde_json::json!({
            "command": command,
            "params": params,
        });

        let output = self
            .plugin
            .call_guest("handle_command", &input.to_string())
            .map_err(|e| format!("Plugin call failed: {}", e))?;

        let response: JsonValue =
            serde_json::from_str(&output).map_err(|e| format!("Invalid plugin response: {}", e))?;

        if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
            Ok(response.get("data").cloned().unwrap_or(JsonValue::Null))
        } else {
            let error_msg = response
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown plugin error");
            Err(error_msg.to_string())
        }
    }
}

// ============================================================================
// Plugin loading helpers
// ============================================================================

/// Find the path to a plugin's WASM file.
///
/// Search order:
/// 1. `~/.diaryx/plugins/{plugin_id}/plugin.wasm` (user-installed)
/// 2. Compile-time bundled (future: `include_bytes!` fallback)
fn find_plugin_wasm(plugin_id: &str) -> Result<PathBuf, String> {
    // Check user plugins directory
    if let Some(home) = dirs::home_dir() {
        let user_path = home
            .join(".diaryx")
            .join("plugins")
            .join(plugin_id)
            .join("plugin.wasm");
        if user_path.exists() {
            return Ok(user_path);
        }
    }

    // Check XDG data directory
    if let Some(data_dir) = dirs::data_dir() {
        let xdg_path = data_dir
            .join("diaryx")
            .join("plugins")
            .join(plugin_id)
            .join("plugin.wasm");
        if xdg_path.exists() {
            return Ok(xdg_path);
        }
    }

    Err(format!(
        "Plugin '{}' not found. Install it to ~/.diaryx/plugins/{}/plugin.wasm",
        plugin_id, plugin_id
    ))
}

/// Create a `HostContext` for the CLI with file-based storage.
fn create_host_context(workspace_root: &Path) -> Arc<HostContext> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let storage = Arc::new(CliPluginStorage::new(workspace_root));
    let event_emitter = Arc::new(CliEventEmitter);

    Arc::new(HostContext {
        fs: Arc::new(fs),
        storage,
        event_emitter,
    })
}

/// Load the sync WASM plugin.
fn load_sync_plugin(workspace_root: &Path) -> Result<ExtismPluginAdapter, String> {
    let wasm_path = find_plugin_wasm("diaryx.sync")?;
    let host_context = create_host_context(workspace_root);

    load_plugin_from_wasm(&wasm_path, host_context, None)
        .map_err(|e| format!("Failed to load sync plugin: {}", e))
}

/// Load the publish WASM plugin.
fn load_publish_plugin(workspace_root: &Path) -> Result<ExtismPluginAdapter, String> {
    let wasm_path = find_plugin_wasm("diaryx.publish")?;
    let host_context = create_host_context(workspace_root);

    load_plugin_from_wasm(&wasm_path, host_context, None)
        .map_err(|e| format!("Failed to load publish plugin: {}", e))
}
