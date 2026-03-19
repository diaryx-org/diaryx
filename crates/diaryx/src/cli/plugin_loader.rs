//! Plugin loading and context for CLI Extism integration.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::auth::{AuthCredentials, NativeFileAuthStorage};
use diaryx_core::config::Config;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::{Plugin, PluginContext, PluginManifest};
use diaryx_extism::protocol::GuestManifest;
use diaryx_extism::{
    EventEmitter, ExtismPluginAdapter, FrontmatterPermissionChecker, HostContext,
    TokioWebSocketBridge, load_plugin_from_wasm,
};
use serde_json::Value as JsonValue;

use super::plugin_storage::CliPluginStorage;

struct CliRuntimeContextProvider {
    workspace_root: PathBuf,
}

impl CliRuntimeContextProvider {
    fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }
}

impl diaryx_extism::RuntimeContextProvider for CliRuntimeContextProvider {
    fn get_context(&self, plugin_id: &str) -> JsonValue {
        build_runtime_context(Config::load().ok(), &self.workspace_root, plugin_id)
    }
}

fn build_runtime_context(
    config: Option<Config>,
    workspace_root: &Path,
    plugin_id: &str,
) -> JsonValue {
    build_runtime_context_from_sources(
        config,
        NativeFileAuthStorage::load_global_credentials(),
        workspace_root,
        plugin_id,
    )
}

fn build_runtime_context_from_sources(
    config: Option<Config>,
    auth_credentials: Option<AuthCredentials>,
    workspace_root: &Path,
    plugin_id: &str,
) -> JsonValue {
    let workspace_root_path = workspace_root.to_path_buf();
    let workspace_root = workspace_root_path.display().to_string();

    let current_workspace_id = config.as_ref().and_then(|config| {
        config
            .workspace_registry()
            .find_by_path(&workspace_root_path)
            .map(|entry| entry.id.clone())
    });

    let provider_links = if plugin_id == "diaryx.sync" {
        auth_credentials
            .as_ref()
            .and_then(|credentials| credentials.workspace_id.as_ref())
            .as_ref()
            .map(|remote_workspace_id| {
                vec![serde_json::json!({
                    "pluginId": plugin_id,
                    "remoteWorkspaceId": remote_workspace_id,
                })]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let server_url = auth_credentials
        .as_ref()
        .map(|credentials| credentials.server_url.clone())
        .unwrap_or_else(|| diaryx_core::auth::DEFAULT_SYNC_SERVER.to_string());
    let auth_token = auth_credentials.and_then(|credentials| credentials.session_token);

    serde_json::json!({
        "server_url": server_url,
        "auth_token": auth_token,
        "current_workspace": {
            "id": current_workspace_id,
            "path": workspace_root,
            "provider_links": provider_links,
        }
    })
}

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
                "status_changed" => {}
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
                _ => {}
            }
        }
    }
}

/// Publish plugin context for the CLI.
///
/// Wraps an `ExtismPluginAdapter` loaded from `diaryx_publish.wasm`.
pub struct CliPublishContext {
    plugin: Arc<ExtismPluginAdapter>,
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
    plugin: Arc<ExtismPluginAdapter>,
}

impl CliPluginContext {
    /// Load a plugin context for a specific plugin ID.
    ///
    /// Requires canonical namespaced IDs (e.g. `diaryx.publish`).
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

/// Find plugin.wasm using the canonical plugin ID.
fn find_plugin_wasm(plugin_id: &str) -> Result<PathBuf, String> {
    find_plugin_wasm_exact(plugin_id).map_err(|_| {
        format!(
            "Plugin '{}' not found. Install it with canonical ID: diaryx plugin install {}",
            plugin_id, plugin_id
        )
    })
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
fn create_host_context(
    workspace_root: &Path,
    plugin_id: &str,
    ws_bridge: Arc<dyn diaryx_extism::WebSocketBridge>,
) -> Arc<HostContext> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let storage = Arc::new(CliPluginStorage::new(workspace_root));
    let event_emitter = Arc::new(CliEventEmitter);

    Arc::new(HostContext {
        fs: Arc::new(fs),
        storage,
        secret_store: Arc::new(diaryx_extism::FilePluginSecretStore::new(
            workspace_root.join(".diaryx").join("plugin-secrets"),
        )),
        event_emitter,
        plugin_id: plugin_id.to_string(),
        permission_checker: Some(Arc::new(FrontmatterPermissionChecker::from_workspace_root(
            Some(workspace_root.to_path_buf()),
        ))),
        file_provider: Arc::new(diaryx_extism::NoopFileProvider),
        ws_bridge,
        plugin_command_bridge: Arc::new(diaryx_extism::NoopPluginCommandBridge),
        runtime_context_provider: Arc::new(CliRuntimeContextProvider::new(workspace_root)),
        namespace_provider: Arc::new(diaryx_extism::NoopNamespaceProvider),
    })
}

/// Load an arbitrary WASM plugin by ID.
fn load_plugin(workspace_root: &Path, plugin_id: &str) -> Result<Arc<ExtismPluginAdapter>, String> {
    let wasm_path = find_plugin_wasm(plugin_id)?;
    let ws_bridge = Arc::new(TokioWebSocketBridge::new());
    let host_context = create_host_context(workspace_root, plugin_id, ws_bridge.clone());

    let plugin = Arc::new(
        load_plugin_from_wasm(&wasm_path, host_context, None)
            .map_err(|e| format!("Failed to load plugin '{}': {}", plugin_id, e))?,
    );

    let plugin_bridge: Arc<dyn diaryx_extism::SyncGuestBridge> = plugin.clone();
    ws_bridge.set_guest_bridge(Arc::downgrade(&plugin_bridge));

    // Initialize plugin with workspace context so guest plugins can resolve
    // workspace-scoped paths and config deterministically.
    let ctx = PluginContext {
        workspace_root: Some(workspace_root.to_path_buf()),
        link_format: diaryx_core::link_parser::LinkFormat::default(),
    };
    futures_lite::future::block_on(Plugin::init(plugin.as_ref(), &ctx))
        .map_err(|e| format!("Failed to initialize plugin '{}': {}", plugin_id, e))?;

    Ok(plugin)
}

/// Load the publish WASM plugin.
fn load_publish_plugin(workspace_root: &Path) -> Result<Arc<ExtismPluginAdapter>, String> {
    load_plugin(workspace_root, "diaryx.publish")
}

#[cfg(test)]
mod tests {
    use super::build_runtime_context_from_sources;
    use diaryx_core::auth::AuthCredentials;
    use diaryx_core::config::Config;
    use diaryx_core::workspace_registry::WorkspaceEntry;
    use std::path::{Path, PathBuf};

    #[test]
    fn runtime_context_includes_sync_link_for_sync_plugin() {
        let workspace_root = Path::new("/tmp/diaryx-workspace");
        let mut config = Config::new(PathBuf::from(workspace_root));
        config.workspaces.push(WorkspaceEntry {
            id: "local-1".into(),
            name: "workspace".into(),
            path: Some(PathBuf::from(workspace_root)),
        });

        let auth_credentials = Some(AuthCredentials {
            server_url: "https://sync.example.com".into(),
            session_token: Some("session-token".into()),
            email: Some("user@example.com".into()),
            workspace_id: Some("remote-123".into()),
        });

        let context = build_runtime_context_from_sources(
            Some(config),
            auth_credentials,
            workspace_root,
            "diaryx.sync",
        );

        assert_eq!(
            context.get("server_url").and_then(|v| v.as_str()),
            Some("https://sync.example.com")
        );
        assert_eq!(
            context.get("auth_token").and_then(|v| v.as_str()),
            Some("session-token")
        );
        assert_eq!(
            context
                .get("current_workspace")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str()),
            Some("local-1")
        );
        assert_eq!(
            context
                .get("current_workspace")
                .and_then(|v| v.get("provider_links"))
                .and_then(|v| v.as_array())
                .map(|links| links.len()),
            Some(1)
        );
    }

    #[test]
    fn runtime_context_omits_sync_link_for_non_sync_plugins() {
        let workspace_root = Path::new("/tmp/diaryx-workspace");
        let config = Config::new(PathBuf::from(workspace_root));
        let auth_credentials = Some(AuthCredentials {
            server_url: "https://sync.example.com".into(),
            session_token: Some("session-token".into()),
            email: None,
            workspace_id: Some("remote-123".into()),
        });

        let context = build_runtime_context_from_sources(
            Some(config),
            auth_credentials,
            workspace_root,
            "diaryx.publish",
        );

        assert_eq!(
            context
                .get("current_workspace")
                .and_then(|v| v.get("provider_links"))
                .and_then(|v| v.as_array())
                .map(|links| links.len()),
            Some(0)
        );
    }

    #[test]
    fn runtime_context_uses_default_server_without_auth() {
        let workspace_root = Path::new("/tmp/diaryx-workspace");
        let config = Config::new(PathBuf::from(workspace_root));
        let context =
            build_runtime_context_from_sources(Some(config), None, workspace_root, "diaryx.sync");

        assert_eq!(
            context.get("server_url").and_then(|v| v.as_str()),
            Some(diaryx_core::auth::DEFAULT_SYNC_SERVER)
        );
        assert!(context.get("auth_token").is_some());
        assert!(context.get("auth_token").unwrap().is_null());
    }
}
