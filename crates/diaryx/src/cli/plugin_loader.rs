//! Plugin loading and context for CLI Extism integration.
//!
//! Provides `CliSyncContext` which replaces the old `CrdtContext` for all
//! sync plugin interactions, routing through Extism plugin commands instead
//! of direct CRDT API calls.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::{Plugin, PluginContext, PluginManifest};
use diaryx_extism::protocol::GuestManifest;
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

/// Generic plugin context for CLI-dispatched plugin commands.
///
/// Wraps an `ExtismPluginAdapter` loaded for an arbitrary plugin ID.
pub struct CliPluginContext {
    plugin: ExtismPluginAdapter,
}

impl CliPluginContext {
    /// Load a plugin context for a specific plugin ID.
    ///
    /// Accepts both namespaced IDs (e.g. `diaryx.publish`) and short IDs
    /// (e.g. `publish`) when resolving plugin directory names.
    pub fn load(workspace_root: &Path, plugin_id: &str) -> Result<Self, String> {
        let plugin = load_plugin(workspace_root, plugin_id)?;
        Ok(Self { plugin })
    }

    /// Send a command to the plugin and return the result.
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
/// 1. `~/.diaryx/plugins/{id}.diaryx/plugin.wasm` (new convention)
/// 2. `~/.diaryx/plugins/{id}/plugin.wasm` (legacy)
/// 3. `$XDG_DATA_HOME/diaryx/plugins/{id}/plugin.wasm` (Tauri compat)
fn find_plugin_wasm_exact(plugin_id: &str) -> Result<PathBuf, String> {
    // Check user plugins directory (new .diaryx extension convention)
    if let Some(home) = dirs::home_dir() {
        let new_path = home
            .join(".diaryx")
            .join("plugins")
            .join(format!("{}.diaryx", plugin_id))
            .join("plugin.wasm");
        if new_path.exists() {
            return Ok(new_path);
        }

        // Legacy path without .diaryx extension
        let legacy_path = home
            .join(".diaryx")
            .join("plugins")
            .join(plugin_id)
            .join("plugin.wasm");
        if legacy_path.exists() {
            return Ok(legacy_path);
        }
    }

    // Check XDG data directory (Tauri compat)
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

    Err(format!("Plugin '{}' not found", plugin_id))
}

/// Find plugin.wasm using common ID variants.
fn find_plugin_wasm(plugin_id: &str) -> Result<PathBuf, String> {
    let mut candidates = vec![plugin_id.to_string()];
    if let Some(stripped) = plugin_id.strip_prefix("diaryx.") {
        candidates.push(stripped.to_string());
    } else {
        candidates.push(format!("diaryx.{plugin_id}"));
    }

    for candidate in candidates {
        if let Ok(path) = find_plugin_wasm_exact(&candidate) {
            return Ok(path);
        }
    }

    Err(format!(
        "Plugin '{}' not found. Install it with: diaryx plugin install {}",
        plugin_id, plugin_id
    ))
}

/// Discover installed plugin manifests by scanning cached `manifest.json` files.
///
/// This is fast — no WASM loading, just JSON file reads.
/// Returns `(plugin_id, PluginManifest)` pairs for every installed plugin
/// that has a cached manifest.
pub fn discover_plugin_manifests() -> Vec<(String, PluginManifest)> {
    let mut results = Vec::new();

    let dirs_to_scan: Vec<PathBuf> = {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".diaryx").join("plugins"));
        }
        if let Some(data_dir) = dirs::data_dir() {
            let xdg = data_dir.join("diaryx").join("plugins");
            if !dirs.contains(&xdg) {
                dirs.push(xdg);
            }
        }
        dirs
    };

    for dir in &dirs_to_scan {
        if !dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Must have plugin.wasm to be a valid plugin
            if !path.join("plugin.wasm").exists() {
                continue;
            }
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }

            let json = match std::fs::read_to_string(&manifest_path) {
                Ok(j) => j,
                Err(_) => continue,
            };

            // Try parsing as PluginManifest first (the format we cache).
            if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&json) {
                let id = manifest.id.0.clone();
                if !results.iter().any(|(existing_id, _)| existing_id == &id) {
                    results.push((id, manifest));
                }
                continue;
            }

            // Fall back to GuestManifest format (legacy cached format).
            if let Ok(guest) = serde_json::from_str::<GuestManifest>(&json) {
                let manifest = convert_guest_manifest_to_plugin(&guest);
                let id = manifest.id.0.clone();
                if !results.iter().any(|(existing_id, _)| existing_id == &id) {
                    results.push((id, manifest));
                }
            }
        }
    }

    results
}

/// Minimal conversion from GuestManifest to PluginManifest for discovery.
/// Only populates the fields needed for CLI command building.
fn convert_guest_manifest_to_plugin(guest: &GuestManifest) -> PluginManifest {
    use diaryx_core::plugin::PluginId;

    let cli = guest
        .cli
        .iter()
        .filter_map(|val| serde_json::from_value(val.clone()).ok())
        .collect();

    PluginManifest {
        id: PluginId(guest.id.clone()),
        name: guest.name.clone(),
        version: guest.version.clone(),
        description: guest.description.clone(),
        capabilities: vec![],
        ui: vec![],
        cli,
    }
}

/// Create a `HostContext` for the CLI with file-based storage.
fn create_host_context(workspace_root: &Path, plugin_id: &str) -> Arc<HostContext> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let storage = Arc::new(CliPluginStorage::new(workspace_root));
    let event_emitter = Arc::new(CliEventEmitter);

    Arc::new(HostContext {
        fs: Arc::new(fs),
        storage,
        event_emitter,
        plugin_id: plugin_id.to_string(),
        permission_checker: None,
    })
}

/// Load an arbitrary WASM plugin by ID.
fn load_plugin(workspace_root: &Path, plugin_id: &str) -> Result<ExtismPluginAdapter, String> {
    let wasm_path = find_plugin_wasm(plugin_id)?;
    let host_context = create_host_context(workspace_root, plugin_id);

    let plugin = load_plugin_from_wasm(&wasm_path, host_context, None)
        .map_err(|e| format!("Failed to load plugin '{}': {}", plugin_id, e))?;

    // Initialize plugin with workspace context so guest plugins can resolve
    // workspace-scoped paths and config deterministically.
    let ctx = PluginContext {
        workspace_root: Some(workspace_root.to_path_buf()),
        link_format: diaryx_core::link_parser::LinkFormat::default(),
    };
    futures_lite::future::block_on(Plugin::init(&plugin, &ctx))
        .map_err(|e| format!("Failed to initialize plugin '{}': {}", plugin_id, e))?;

    Ok(plugin)
}

/// Load the sync WASM plugin.
fn load_sync_plugin(workspace_root: &Path) -> Result<ExtismPluginAdapter, String> {
    load_plugin(workspace_root, "diaryx.sync")
}

/// Load the publish WASM plugin.
fn load_publish_plugin(workspace_root: &Path) -> Result<ExtismPluginAdapter, String> {
    load_plugin(workspace_root, "diaryx.publish")
}
