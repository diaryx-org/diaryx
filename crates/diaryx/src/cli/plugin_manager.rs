//! Plugin management commands — install, remove, list, search, update, info.
//!
//! Downloads plugins from the Diaryx CDN registry and manages the local
//! plugin directory at `~/.diaryx/plugins/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_extism::{HostContext, load_plugin_from_wasm};
use serde::Deserialize;

use crate::cli::args::PluginCommands;

const REGISTRY_URL: &str = "https://cdn.diaryx.org/plugins/plugins.json";

#[derive(Debug, Deserialize)]
struct RegistryPlugin {
    id: String,
    name: String,
    description: String,
    version: String,
    wasm_url: String,
    #[serde(default)]
    builtin: bool,
}

/// Handle the `diaryx plugin <subcommand>` family.
pub fn handle_plugin_command(command: PluginCommands) {
    match command {
        PluginCommands::List => handle_list(),
        PluginCommands::Install { id } => handle_install(&id),
        PluginCommands::Remove { id, yes } => handle_remove(&id, yes),
        PluginCommands::Search { query } => handle_search(query.as_deref()),
        PluginCommands::Update { id } => handle_update(id.as_deref()),
        PluginCommands::Info { id } => handle_info(&id),
    }
}

/// Return the plugins directory (`~/.diaryx/plugins/`).
fn plugins_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".diaryx")
        .join("plugins")
}

/// Return the plugin directory for a given ID.
fn plugin_dir(id: &str) -> PathBuf {
    plugins_dir().join(format!("{}.diaryx", id))
}

/// List installed plugins by scanning manifest.json files.
fn handle_list() {
    let dir = plugins_dir();
    if !dir.exists() {
        println!("No plugins installed.");
        println!();
        println!("Install default plugins with:");
        println!("  diaryx plugin install --defaults");
        return;
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => {
            println!("No plugins installed.");
            return;
        }
    };

    let mut found = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            // Check for plugin.wasm without manifest
            if path.join("plugin.wasm").exists() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                println!("  {} (manifest not cached)", name);
                found = true;
            }
            continue;
        }

        if let Ok(json) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&json) {
                let id = manifest.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = manifest.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let version = manifest
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                println!("  {:<20} {:<20} v{}", id, name, version);
                found = true;
            }
        }
    }

    if !found {
        println!("No plugins installed.");
        println!();
        println!("Install default plugins with:");
        println!("  diaryx plugin install --defaults");
    }
}

/// Install a plugin from the registry.
fn handle_install(id: &str) {
    let registry = match fetch_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to fetch plugin registry: {}", e);
            return;
        }
    };

    if id == "--defaults" {
        let builtins: Vec<&RegistryPlugin> = registry.iter().filter(|p| p.builtin).collect();
        if builtins.is_empty() {
            println!("No built-in plugins found in registry.");
            return;
        }
        println!("Installing {} default plugin(s)...", builtins.len());
        for plugin in &builtins {
            install_plugin(plugin);
        }
        println!("Done.");
        return;
    }

    match registry.iter().find(|p| p.id == id) {
        Some(plugin) => install_plugin(plugin),
        None => {
            eprintln!("Plugin '{}' not found in registry.", id);
            eprintln!();
            eprintln!("Search available plugins with:");
            eprintln!("  diaryx plugin search");
        }
    }
}

/// Download and install a single plugin.
fn install_plugin(plugin: &RegistryPlugin) {
    let dest = plugin_dir(&plugin.id);

    // Create directory
    if let Err(e) = std::fs::create_dir_all(&dest) {
        eprintln!("Failed to create plugin directory: {}", e);
        return;
    }

    let wasm_path = dest.join("plugin.wasm");
    println!("  Installing {} v{}...", plugin.name, plugin.version);

    // Download WASM
    match download_file(&plugin.wasm_url, &wasm_path) {
        Ok(size) => {
            println!(
                "    Downloaded {} ({:.1} KB)",
                plugin.id,
                size as f64 / 1024.0
            );
        }
        Err(e) => {
            eprintln!("    Failed to download {}: {}", plugin.id, e);
            // Clean up on failure
            let _ = std::fs::remove_dir_all(&dest);
            return;
        }
    }

    // Cache the real plugin manifest immediately so dynamic CLI commands
    // become available right after install.
    if let Err(err) = cache_manifest_from_wasm(&wasm_path) {
        eprintln!("    Warning: failed to cache plugin manifest: {}", err);
        // Fallback to a basic manifest so the plugin still appears in list/info.
        let basic_manifest = serde_json::json!({
            "id": plugin.id,
            "name": plugin.name,
            "version": plugin.version,
            "description": plugin.description,
            "capabilities": [],
            "ui": [],
            "commands": [],
            "cli": [],
        });
        let manifest_path = dest.join("manifest.json");
        let _ = std::fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&basic_manifest).unwrap_or_default(),
        );
    }

    println!("    Installed to {}", dest.display());
}

/// Load a plugin once to trigger manifest.json cache generation.
fn cache_manifest_from_wasm(wasm_path: &Path) -> Result<(), String> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let host_context = Arc::new(HostContext::with_fs(Arc::new(fs)));
    load_plugin_from_wasm(wasm_path, host_context, None)
        .map(|_| ())
        .map_err(|e| format!("{}", e))
}

/// Remove an installed plugin.
fn handle_remove(id: &str, yes: bool) {
    let dest = plugin_dir(id);
    if !dest.exists() {
        eprintln!("Plugin '{}' is not installed.", id);
        return;
    }

    if !yes {
        use std::io::{self, Write};
        print!("Remove plugin '{}'? [y/N] ", id);
        io::stdout().flush().unwrap();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("Failed to read input");
            return;
        }
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("Cancelled.");
            return;
        }
    }

    match std::fs::remove_dir_all(&dest) {
        Ok(()) => println!("Removed plugin '{}'.", id),
        Err(e) => eprintln!("Failed to remove plugin: {}", e),
    }
}

/// Search the plugin registry.
fn handle_search(query: Option<&str>) {
    let registry = match fetch_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to fetch plugin registry: {}", e);
            return;
        }
    };

    let matches: Vec<&RegistryPlugin> = if let Some(q) = query {
        let q_lower = q.to_lowercase();
        registry
            .iter()
            .filter(|p| {
                p.id.to_lowercase().contains(&q_lower)
                    || p.name.to_lowercase().contains(&q_lower)
                    || p.description.to_lowercase().contains(&q_lower)
            })
            .collect()
    } else {
        registry.iter().collect()
    };

    if matches.is_empty() {
        println!("No plugins found.");
        return;
    }

    let installed_dir = plugins_dir();
    for plugin in &matches {
        let installed = plugin_dir(&plugin.id).exists()
            || installed_dir.join(&plugin.id).join("plugin.wasm").exists();
        let status = if installed { " [installed]" } else { "" };
        let builtin = if plugin.builtin { " (built-in)" } else { "" };
        println!(
            "  {:<20} v{:<10} {}{}{}",
            plugin.id, plugin.version, plugin.description, builtin, status
        );
    }
}

/// Update installed plugins.
fn handle_update(specific_id: Option<&str>) {
    let registry = match fetch_registry() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to fetch plugin registry: {}", e);
            return;
        }
    };

    let dir = plugins_dir();
    if !dir.exists() {
        println!("No plugins installed.");
        return;
    }

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();

    let mut updated = 0;
    for entry in entries {
        let path = entry.path();
        let dir_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Strip .diaryx extension for ID lookup
        let plugin_id = dir_name.strip_suffix(".diaryx").unwrap_or(&dir_name);

        if let Some(specific) = specific_id {
            if plugin_id != specific {
                continue;
            }
        }

        let manifest_path = path.join("manifest.json");
        let installed_version = if manifest_path.exists() {
            std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|json| {
                    serde_json::from_str::<serde_json::Value>(&json)
                        .ok()?
                        .get("version")?
                        .as_str()
                        .map(String::from)
                })
        } else {
            None
        };

        if let Some(registry_plugin) = registry.iter().find(|p| p.id == plugin_id) {
            let needs_update = installed_version
                .as_deref()
                .map_or(true, |v| v != registry_plugin.version);

            if needs_update {
                println!(
                    "  Updating {} {} -> {}",
                    plugin_id,
                    installed_version.as_deref().unwrap_or("?"),
                    registry_plugin.version
                );
                install_plugin(registry_plugin);
                updated += 1;
            }
        }
    }

    if updated == 0 {
        println!("All plugins are up to date.");
    } else {
        println!("Updated {} plugin(s).", updated);
    }
}

/// Show details about an installed plugin.
fn handle_info(id: &str) {
    let dest = plugin_dir(id);
    let manifest_path = dest.join("manifest.json");

    if !manifest_path.exists() {
        // Try legacy path without .diaryx extension
        let legacy = plugins_dir().join(id).join("manifest.json");
        if legacy.exists() {
            print_manifest_info(&legacy);
            return;
        }
        eprintln!(
            "Plugin '{}' is not installed or has no cached manifest.",
            id
        );
        return;
    }

    print_manifest_info(&manifest_path);
}

fn print_manifest_info(manifest_path: &Path) {
    match std::fs::read_to_string(manifest_path) {
        Ok(json) => match serde_json::from_str::<serde_json::Value>(&json) {
            Ok(manifest) => {
                let id = manifest.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let name = manifest.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let version = manifest
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let description = manifest
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                println!("Plugin: {} ({})", name, id);
                println!("Version: {}", version);
                println!("Description: {}", description);

                if let Some(caps) = manifest.get("capabilities").and_then(|v| v.as_array()) {
                    if !caps.is_empty() {
                        println!(
                            "Capabilities: {}",
                            caps.iter()
                                .filter_map(|c| c.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }

                if let Some(cmds) = manifest.get("commands").and_then(|v| v.as_array()) {
                    if !cmds.is_empty() {
                        println!("Commands: {}", cmds.len());
                    }
                }

                if let Some(cli) = manifest.get("cli").and_then(|v| v.as_array()) {
                    if !cli.is_empty() {
                        println!("CLI commands:");
                        for cmd in cli {
                            if let Some(name) = cmd.get("name").and_then(|v| v.as_str()) {
                                let about = cmd.get("about").and_then(|v| v.as_str()).unwrap_or("");
                                println!("  {:<20} {}", name, about);
                            }
                        }
                    }
                }

                println!(
                    "Location: {}",
                    manifest_path.parent().unwrap_or(Path::new(".")).display()
                );
            }
            Err(e) => eprintln!("Failed to parse manifest: {}", e),
        },
        Err(e) => eprintln!("Failed to read manifest: {}", e),
    }
}

// ============================================================================
// HTTP helpers
// ============================================================================

/// Fetch and parse the plugin registry.
fn fetch_registry() -> Result<Vec<RegistryPlugin>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(REGISTRY_URL)
        .send()
        .map_err(|e| format!("Failed to fetch registry: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Registry returned status {}", response.status()));
    }

    let plugins: Vec<RegistryPlugin> = response
        .json()
        .map_err(|e| format!("Failed to parse registry: {}", e))?;

    Ok(plugins)
}

/// Download a file from a URL to a local path. Returns file size in bytes.
fn download_file(url: &str, dest: &Path) -> Result<usize, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download returned status {}", response.status()));
    }

    let bytes = response
        .bytes()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    std::fs::write(dest, &bytes).map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(bytes.len())
}
