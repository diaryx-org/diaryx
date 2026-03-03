//! Plugin loader — scans a directory for WASM plugins and loads them.
//!
//! Expected directory structure:
//! ```text
//! ~/.diaryx/plugins/
//!   my-plugin/
//!     plugin.wasm      # The WASM module
//!     manifest.json    # Optional cached manifest (skip calling guest fn)
//!     config.json      # Plugin config (created/updated at runtime)
//! ```

use std::path::Path;
use std::sync::Arc;

use diaryx_core::plugin::Plugin;
use extism::{Manifest as ExtismManifest, PluginBuilder, UserData, Wasm};
use thiserror::Error;

use crate::adapter::ExtismPluginAdapter;
use crate::host_fns::{self, HostContext};
use crate::protocol::GuestManifest;

/// Errors that can occur during plugin loading.
#[derive(Debug, Error)]
pub enum ExtismLoadError {
    /// Failed to read the plugins directory.
    #[error("Failed to read plugins directory: {0}")]
    ReadDir(#[from] std::io::Error),

    /// Failed to create an Extism plugin from the WASM module.
    #[error("Failed to create Extism plugin '{plugin_name}': {source}")]
    PluginCreate {
        plugin_name: String,
        source: extism::Error,
    },

    /// Failed to call the guest's `manifest` function.
    #[error("Failed to get manifest from plugin '{plugin_name}': {source}")]
    ManifestCall {
        plugin_name: String,
        source: extism::Error,
    },

    /// Failed to parse the guest manifest JSON.
    #[error("Failed to parse manifest from plugin '{plugin_name}': {source}")]
    ManifestParse {
        plugin_name: String,
        source: serde_json::Error,
    },
}

/// Load all WASM plugins from the given directory.
///
/// Scans `plugins_dir` for subdirectories containing a `plugin.wasm` file.
/// For each valid plugin:
/// 1. Creates an Extism plugin with registered host functions
/// 2. Calls the guest's `manifest` export (or reads `manifest.json` cache)
/// 3. Loads `config.json` if present
/// 4. Returns an [`ExtismPluginAdapter`] ready for registration
///
/// Plugins that fail to load are logged and skipped (not fatal).
pub fn load_plugins_from_dir(
    plugins_dir: &Path,
    host_context: Arc<HostContext>,
) -> Result<Vec<ExtismPluginAdapter>, ExtismLoadError> {
    let mut adapters = Vec::new();

    let entries = std::fs::read_dir(plugins_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let wasm_path = path.join("plugin.wasm");
        if !wasm_path.exists() {
            continue;
        }

        let plugin_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".into());

        match load_single_plugin(&path, &wasm_path, &plugin_name, &host_context) {
            Ok(adapter) => {
                log::info!(
                    "Loaded extism plugin: {} ({})",
                    adapter.manifest().name,
                    adapter.manifest().id
                );
                adapters.push(adapter);
            }
            Err(e) => {
                log::warn!("Failed to load plugin from {}: {e}", path.display());
            }
        }
    }

    Ok(adapters)
}

/// Load a single WASM plugin from a file path with a given host context.
///
/// This is a lower-level API for loading a specific plugin (e.g. the sync plugin)
/// rather than scanning a directory. The caller provides the WASM file path,
/// host context, and an optional config JSON sidecar path.
///
/// # Arguments
/// * `wasm_path` — Path to the `.wasm` file
/// * `host_context` — Host functions context (filesystem, storage, events)
/// * `config_path` — Optional path to config.json sidecar. If `None`, uses
///   a sibling `config.json` next to the WASM file.
pub fn load_plugin_from_wasm(
    wasm_path: &Path,
    host_context: Arc<HostContext>,
    config_path: Option<&Path>,
) -> Result<ExtismPluginAdapter, ExtismLoadError> {
    let plugin_name = wasm_path
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".into());

    let wasm = Wasm::file(wasm_path);
    let extism_manifest = ExtismManifest::new([wasm]);
    let user_data = UserData::new(HostContext {
        fs: host_context.fs.clone(),
        storage: host_context.storage.clone(),
        event_emitter: host_context.event_emitter.clone(),
        plugin_id: plugin_name.clone(),
        permission_checker: host_context.permission_checker.clone(),
    });

    let builder = PluginBuilder::new(extism_manifest).with_wasi(true);
    let builder = host_fns::register_host_functions(builder, user_data.clone());
    let mut plugin = builder.build().map_err(|e| ExtismLoadError::PluginCreate {
        plugin_name: plugin_name.clone(),
        source: e,
    })?;

    // Call the guest's manifest export.
    let output =
        plugin
            .call::<&str, &[u8]>("manifest", "")
            .map_err(|e| ExtismLoadError::ManifestCall {
                plugin_name: plugin_name.clone(),
                source: e,
            })?;
    let output_str = String::from_utf8_lossy(output);
    let guest_manifest = serde_json::from_str::<GuestManifest>(&output_str).map_err(|e| {
        ExtismLoadError::ManifestParse {
            plugin_name: plugin_name.clone(),
            source: e,
        }
    })?;
    if let Ok(ctx) = user_data.get()
        && let Ok(mut guard) = ctx.lock()
    {
        guard.plugin_id = guest_manifest.id.clone();
    }

    // Cache the manifest.json alongside the WASM for fast discovery.
    let manifest_path = wasm_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("manifest.json");
    cache_manifest(&manifest_path, &guest_manifest);

    // Load config sidecar.
    let cfg_path = config_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        wasm_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("config.json")
    });
    let config = if cfg_path.exists() {
        let json = std::fs::read_to_string(&cfg_path).map_err(ExtismLoadError::ReadDir)?;
        serde_json::from_str(&json).unwrap_or(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::Value::Object(Default::default())
    };

    Ok(ExtismPluginAdapter::new(
        plugin,
        guest_manifest,
        config,
        cfg_path,
    ))
}

/// Load a single plugin from its directory.
fn load_single_plugin(
    plugin_dir: &Path,
    wasm_path: &Path,
    plugin_name: &str,
    host_context: &Arc<HostContext>,
) -> Result<ExtismPluginAdapter, ExtismLoadError> {
    let wasm = Wasm::file(wasm_path);
    let extism_manifest = ExtismManifest::new([wasm]);
    let user_data = UserData::new(HostContext {
        fs: host_context.fs.clone(),
        storage: host_context.storage.clone(),
        event_emitter: host_context.event_emitter.clone(),
        plugin_id: plugin_name.to_string(),
        permission_checker: host_context.permission_checker.clone(),
    });

    let builder = PluginBuilder::new(extism_manifest).with_wasi(true);
    let builder = host_fns::register_host_functions(builder, user_data.clone());
    let mut plugin = builder.build().map_err(|e| ExtismLoadError::PluginCreate {
        plugin_name: plugin_name.into(),
        source: e,
    })?;

    // Try to read a cached manifest.json first; fall back to calling the guest.
    let manifest_path = plugin_dir.join("manifest.json");
    let guest_manifest = if manifest_path.exists() {
        let json = std::fs::read_to_string(&manifest_path).map_err(ExtismLoadError::ReadDir)?;
        serde_json::from_str::<GuestManifest>(&json).map_err(|e| {
            ExtismLoadError::ManifestParse {
                plugin_name: plugin_name.into(),
                source: e,
            }
        })?
    } else {
        let output = plugin.call::<&str, &[u8]>("manifest", "").map_err(|e| {
            ExtismLoadError::ManifestCall {
                plugin_name: plugin_name.into(),
                source: e,
            }
        })?;
        let output_str = String::from_utf8_lossy(output);
        let gm = serde_json::from_str::<GuestManifest>(&output_str).map_err(|e| {
            ExtismLoadError::ManifestParse {
                plugin_name: plugin_name.into(),
                source: e,
            }
        })?;
        // Cache the manifest for fast discovery on next startup.
        cache_manifest(&manifest_path, &gm);
        gm
    };
    if let Ok(ctx) = user_data.get()
        && let Ok(mut guard) = ctx.lock()
    {
        guard.plugin_id = guest_manifest.id.clone();
    }

    // Load config sidecar.
    let config_path = plugin_dir.join("config.json");
    let config = if config_path.exists() {
        let json = std::fs::read_to_string(&config_path).map_err(ExtismLoadError::ReadDir)?;
        serde_json::from_str(&json).unwrap_or(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::Value::Object(Default::default())
    };

    Ok(ExtismPluginAdapter::new(
        plugin,
        guest_manifest,
        config,
        config_path,
    ))
}

/// Write the guest manifest as a JSON sidecar so the CLI can discover
/// plugin metadata without loading the WASM module.
fn cache_manifest(path: &Path, manifest: &GuestManifest) {
    match serde_json::to_string_pretty(manifest) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                log::debug!("Could not cache manifest to {}: {e}", path.display());
            }
        }
        Err(e) => {
            log::debug!("Could not serialize manifest for caching: {e}");
        }
    }
}
