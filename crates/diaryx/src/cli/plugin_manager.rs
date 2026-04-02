//! Plugin management commands — install, remove, list, search, update, info.
//!
//! Downloads plugins from the Diaryx CDN `registry.md` and manages the
//! local plugin directory at `~/.diaryx/plugins/`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::manifest::{MarketplaceEntry, MarketplaceRegistry};
use diaryx_extism::{HostContext, inspect_plugin_wasm_manifest, load_plugin_from_wasm};
use sha2::{Digest, Sha256};

use crate::cli::args::PluginCommands;

const REGISTRY_URL: &str = "https://app.diaryx.org/cdn/plugins/registry.md";

#[derive(Debug, Clone)]
struct InstalledPlugin {
    id: String,
    name: String,
    version: Option<String>,
    description: Option<String>,
    manifest_path: Option<PathBuf>,
    path: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct DiscoveryFilters {
    query: Option<String>,
    category: Option<String>,
    tag: Option<String>,
    author: Option<String>,
    installed_only: bool,
}

/// Handle the `diaryx plugin <subcommand>` family.
pub fn handle_plugin_command(command: PluginCommands) {
    match command {
        PluginCommands::List {
            category,
            tag,
            author,
            json,
        } => handle_list(
            DiscoveryFilters {
                category,
                tag,
                author,
                ..Default::default()
            },
            json,
        ),
        PluginCommands::Install { id } => handle_install(&id),
        PluginCommands::Remove { id, yes } => handle_remove(&id, yes),
        PluginCommands::Search {
            query,
            category,
            tag,
            author,
            installed,
            json,
        } => handle_search(
            DiscoveryFilters {
                query,
                category,
                tag,
                author,
                installed_only: installed,
            },
            json,
        ),
        PluginCommands::Update { id } => handle_update(id.as_deref()),
        PluginCommands::Info { id, json } => handle_info(&id, json),
        PluginCommands::Dev { id, wasm_path } => handle_dev(&id, &wasm_path),
        PluginCommands::Undev { id } => handle_undev(&id),
    }
}

/// Return the workspace-local plugins directory (first one found walking up from cwd).
fn workspace_plugins_dir() -> PathBuf {
    super::plugin_loader::workspace_plugin_dirs()
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            // Fallback: .diaryx/plugins in cwd
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".diaryx")
                .join("plugins")
        })
}

/// Return the plugin directory for a given ID (workspace-local).
fn plugin_dir(id: &str) -> PathBuf {
    workspace_plugins_dir().join(id)
}

fn read_manifest_json(path: &Path) -> Option<serde_json::Value> {
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&json).ok()
}

fn read_installed_plugins() -> Vec<InstalledPlugin> {
    let mut installed = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for dir in super::plugin_loader::workspace_plugin_dirs() {
        if !dir.exists() {
            continue;
        }
        scan_plugins_in_dir(&dir, &mut installed, &mut seen_ids);
    }

    installed.sort_by(|a, b| a.id.cmp(&b.id));
    installed
}

fn scan_plugins_in_dir(
    dir: &Path,
    installed: &mut Vec<InstalledPlugin>,
    seen_ids: &mut std::collections::HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if !path.join("plugin.wasm").exists() {
            continue;
        }

        let dir_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let inferred_id = dir_name
            .strip_suffix(".diaryx")
            .unwrap_or(dir_name.as_str())
            .to_string();

        let manifest_path = path.join("manifest.json");
        let manifest = read_manifest_json(&manifest_path);

        let id = manifest
            .as_ref()
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .unwrap_or_else(|| inferred_id.clone());
        let name = manifest
            .as_ref()
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .unwrap_or_else(|| inferred_id.clone());
        let version = manifest
            .as_ref()
            .and_then(|m| m.get("version"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let description = manifest
            .as_ref()
            .and_then(|m| m.get("description"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());

        // Skip if we've already seen this plugin from a higher-priority directory
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        installed.push(InstalledPlugin {
            id,
            name,
            version,
            description,
            manifest_path: if manifest_path.exists() {
                Some(manifest_path)
            } else {
                None
            },
            path,
        });
    }
}

fn normalize_optional_filter(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
}

fn matches_registry_filters(
    plugin: &MarketplaceEntry,
    filters: &DiscoveryFilters,
    installed_ids: Option<&HashSet<String>>,
) -> bool {
    let query = normalize_optional_filter(&filters.query);
    let category = normalize_optional_filter(&filters.category);
    let tag = normalize_optional_filter(&filters.tag);
    let author = normalize_optional_filter(&filters.author);

    if let Some(q) = query {
        let haystack = format!(
            "{} {} {} {} {} {} {}",
            plugin.id,
            plugin.name,
            plugin.summary,
            plugin.description,
            plugin.author,
            plugin.categories.join(" "),
            plugin.tags.join(" ")
        )
        .to_lowercase();
        if !haystack.contains(&q) {
            return false;
        }
    }

    if let Some(expected_category) = category
        && !plugin
            .categories
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&expected_category))
    {
        return false;
    }

    if let Some(expected_tag) = tag
        && !plugin
            .tags
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&expected_tag))
    {
        return false;
    }

    if let Some(expected_author) = author
        && !plugin
            .author
            .to_lowercase()
            .contains(expected_author.as_str())
    {
        return false;
    }

    if filters.installed_only && !installed_ids.is_some_and(|ids| ids.contains(plugin.id.as_str()))
    {
        return false;
    }

    true
}

fn normalize_sha256(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    normalized
        .strip_prefix("sha256:")
        .unwrap_or(normalized.as_str())
        .to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn is_canonical_plugin_id(id: &str) -> bool {
    let mut parts = id.split('.');
    let mut part_count = 0usize;

    for part in parts.by_ref() {
        part_count += 1;
        if part.is_empty() {
            return false;
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return false;
        }
    }

    part_count >= 2
}

fn build_http_agent(timeout: std::time::Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn format_http_error(err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(code) => format!("HTTP status {code}"),
        other => other.to_string(),
    }
}

/// List installed plugins.
fn handle_list(filters: DiscoveryFilters, json: bool) {
    let installed = read_installed_plugins();
    if installed.is_empty() {
        println!("No plugins installed.");
        return;
    }

    let registry = fetch_registry().ok();
    let registry_by_id: HashMap<String, MarketplaceEntry> = registry
        .as_ref()
        .map(|r| {
            r.plugins
                .iter()
                .cloned()
                .map(|p| (p.id.clone(), p))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let using_metadata_filter = filters.category.is_some()
        || filters.tag.is_some()
        || filters.author.is_some()
        || filters.query.is_some();

    if using_metadata_filter && registry.is_none() {
        eprintln!("Could not fetch registry metadata. Retry later or remove metadata filters.");
        return;
    }

    let mut rows = Vec::new();
    let installed_ids = installed
        .iter()
        .map(|p| p.id.clone())
        .collect::<HashSet<String>>();

    for local in &installed {
        let registry_plugin = registry_by_id.get(&local.id);
        if using_metadata_filter {
            let Some(registry_plugin) = registry_plugin else {
                continue;
            };
            if !matches_registry_filters(registry_plugin, &filters, Some(&installed_ids)) {
                continue;
            }
        }

        rows.push((local, registry_plugin));
    }

    if rows.is_empty() {
        println!("No installed plugins matched your filters.");
        return;
    }

    if json {
        let out = rows
            .iter()
            .map(|(local, registry_plugin)| {
                serde_json::json!({
                    "id": local.id,
                    "name": local.name,
                    "version": local.version,
                    "description": local.description,
                    "installed": true,
                    "author": registry_plugin.as_ref().map(|p| p.author.clone()),
                    "license": registry_plugin.as_ref().map(|p| p.license.clone()),
                    "repository": registry_plugin.as_ref().and_then(|p| p.repository.clone()),
                    "categories": registry_plugin.as_ref().map(|p| p.categories.clone()).unwrap_or_default(),
                    "tags": registry_plugin.as_ref().map(|p| p.tags.clone()).unwrap_or_default(),
                    "unmanagedLocal": registry_plugin.is_none(),
                    "path": local.path.display().to_string(),
                })
            })
            .collect::<Vec<_>>();

        match serde_json::to_string_pretty(&out) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("Failed to render JSON output: {err}"),
        }
        return;
    }

    for (local, registry_plugin) in rows {
        let version = local.version.as_deref().unwrap_or("?");
        if let Some(registry_plugin) = registry_plugin {
            println!(
                "  {:<24} v{:<10} {:<18} {}",
                local.id, version, registry_plugin.author, registry_plugin.summary
            );
        } else {
            let desc = local
                .description
                .as_deref()
                .unwrap_or("Local unmanaged plugin");
            println!(
                "  {:<24} v{:<10} local/unmanaged {}",
                local.id, version, desc
            );
        }
    }
}

/// Install a plugin from the registry.
fn handle_install(id: &str) {
    if id == "--defaults" {
        eprintln!(
            "'--defaults' was removed. Install plugins by canonical ID (for example: diaryx.sync)."
        );
        return;
    }

    if !is_canonical_plugin_id(id) {
        eprintln!(
            "Invalid plugin ID '{}'. Expected canonical namespaced format (for example: diaryx.sync).",
            id
        );
        return;
    }

    let registry = match fetch_registry() {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("Failed to fetch plugin registry: {err}");
            return;
        }
    };

    let Some(plugin) = registry.plugins.iter().find(|p| p.id == id) else {
        eprintln!("Plugin '{id}' not found in registry.");
        eprintln!("Search available plugins with: diaryx plugin search");
        return;
    };

    if let Err(err) = install_plugin(plugin) {
        eprintln!("Failed to install '{}': {err}", plugin.id);
    }
}

/// Download and install a single plugin.
fn install_plugin(plugin: &MarketplaceEntry) -> Result<(), String> {
    let dest = plugin_dir(&plugin.id);
    let existed = dest.exists();

    std::fs::create_dir_all(&dest).map_err(|err| {
        format!(
            "Failed to create plugin directory '{}': {err}",
            dest.display()
        )
    })?;

    let install_result = (|| {
        println!(
            "Installing {} ({}) v{}...",
            plugin.name, plugin.id, plugin.version
        );

        let bytes = download_bytes(&plugin.artifact.url)?;
        verify_download_integrity(plugin, &bytes)?;

        let wasm_path = dest.join("plugin.wasm");
        std::fs::write(&wasm_path, &bytes).map_err(|err| {
            format!(
                "Failed to write plugin WASM '{}': {err}",
                wasm_path.display()
            )
        })?;

        let inspected = inspect_plugin_wasm_manifest(&wasm_path)
            .map_err(|err| format!("Failed to inspect plugin manifest from WASM: {err}"))?;
        verify_inspected_manifest(plugin, &inspected)?;

        cache_manifest_from_wasm(&wasm_path)?;

        println!("Installed to {}", dest.display());
        Ok(())
    })();

    if let Err(err) = install_result {
        if !existed {
            let _ = std::fs::remove_dir_all(&dest);
        }
        return Err(err);
    }

    Ok(())
}

fn verify_download_integrity(plugin: &MarketplaceEntry, bytes: &[u8]) -> Result<(), String> {
    let expected_sha = normalize_sha256(&plugin.artifact.sha256);
    if expected_sha.is_empty() {
        return Err("Registry artifact.sha256 is empty".to_string());
    }

    let actual_sha = sha256_hex(bytes);
    if actual_sha != expected_sha {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            plugin.id, expected_sha, actual_sha
        ));
    }

    if plugin.artifact.size != bytes.len() as u64 {
        return Err(format!(
            "Artifact size mismatch for {}: expected {} bytes, got {} bytes",
            plugin.id,
            plugin.artifact.size,
            bytes.len()
        ));
    }

    Ok(())
}

fn verify_inspected_manifest(
    registry_plugin: &MarketplaceEntry,
    inspected: &diaryx_extism::protocol::GuestManifest,
) -> Result<(), String> {
    if inspected.id != registry_plugin.id {
        return Err(format!(
            "Manifest ID mismatch: registry={}, wasm={}",
            registry_plugin.id, inspected.id
        ));
    }

    if inspected.version != registry_plugin.version {
        return Err(format!(
            "Manifest version mismatch: registry={}, wasm={}",
            registry_plugin.version, inspected.version
        ));
    }

    if inspected.name != registry_plugin.name {
        return Err(format!(
            "Manifest name mismatch: registry='{}', wasm='{}'",
            registry_plugin.name, inspected.name
        ));
    }

    Ok(())
}

/// Load a plugin once to trigger manifest.json cache generation.
fn cache_manifest_from_wasm(wasm_path: &Path) -> Result<(), String> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let host_context = Arc::new(HostContext::with_fs(Arc::new(fs)));
    load_plugin_from_wasm(wasm_path, host_context, None)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Remove an installed plugin.
fn handle_remove(id: &str, yes: bool) {
    let canonical = plugin_dir(id);
    let legacy = workspace_plugins_dir().join(format!("{}.diaryx", id));
    let dest = if canonical.exists() {
        canonical
    } else if legacy.exists() {
        legacy
    } else {
        eprintln!("Plugin '{id}' is not installed.");
        return;
    };

    if !yes {
        use std::io::{self, Write};
        print!("Remove plugin '{id}'? [y/N] ");
        io::stdout().flush().ok();
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            eprintln!("Failed to read confirmation input");
            return;
        }

        let normalized = input.trim().to_ascii_lowercase();
        if normalized != "y" && normalized != "yes" {
            println!("Cancelled.");
            return;
        }
    }

    match std::fs::remove_dir_all(&dest) {
        Ok(()) => println!("Removed plugin '{id}'."),
        Err(err) => eprintln!("Failed to remove plugin '{id}': {err}"),
    }
}

const DEV_MARKER: &str = ".dev-created";

/// Link a local WASM build for development.
fn handle_dev(id: &str, wasm_path: &Path) {
    let wasm_path = match wasm_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            // Allow dangling symlinks — the target may not be built yet.
            if wasm_path.is_absolute() {
                wasm_path.to_path_buf()
            } else {
                std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(wasm_path)
            }
        }
    };

    let dir = plugin_dir(id);
    let created_dir = !dir.exists();

    if created_dir {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("Failed to create plugin directory: {e}");
            return;
        }
        // Leave a marker so `undev` knows to clean up the directory.
        let _ = std::fs::write(dir.join(DEV_MARKER), b"");
    }

    let plugin_wasm = dir.join("plugin.wasm");

    // Back up an existing real file (not a symlink).
    if plugin_wasm.exists() && !plugin_wasm.is_symlink() {
        let backup = dir.join("plugin.wasm.bak");
        if let Err(e) = std::fs::rename(&plugin_wasm, &backup) {
            eprintln!("Failed to back up existing plugin.wasm: {e}");
            return;
        }
        println!("Backed up plugin.wasm → plugin.wasm.bak");
    }

    // Remove an existing symlink so we can replace it.
    if plugin_wasm.is_symlink() {
        let _ = std::fs::remove_file(&plugin_wasm);
    }

    #[cfg(unix)]
    {
        if let Err(e) = std::os::unix::fs::symlink(&wasm_path, &plugin_wasm) {
            eprintln!("Failed to create symlink: {e}");
            return;
        }
    }

    #[cfg(not(unix))]
    {
        eprintln!("Symlinks are not supported on this platform; copying instead.");
        if let Err(e) = std::fs::copy(&wasm_path, &plugin_wasm) {
            eprintln!("Failed to copy WASM file: {e}");
            return;
        }
    }

    if !wasm_path.exists() {
        println!(
            "Warning: {} does not exist yet. The symlink will work once you build it.",
            wasm_path.display()
        );
    }

    // Re-cache the manifest from the new WASM.
    if wasm_path.exists() {
        if let Err(e) = cache_manifest_from_wasm(&plugin_wasm) {
            eprintln!("Warning: failed to cache manifest: {e}");
        }
    }

    println!("Linked {} → {}", plugin_wasm.display(), wasm_path.display());
}

/// Remove a dev symlink and restore the original plugin.
fn handle_undev(id: &str) {
    let dir = plugin_dir(id);
    if !dir.exists() {
        eprintln!("Plugin '{id}' is not installed.");
        return;
    }

    let plugin_wasm = dir.join("plugin.wasm");
    let backup = dir.join("plugin.wasm.bak");
    let was_dev_created = dir.join(DEV_MARKER).exists();

    if !plugin_wasm.is_symlink() {
        eprintln!("Plugin '{id}' is not in dev mode (plugin.wasm is not a symlink).");
        return;
    }

    let _ = std::fs::remove_file(&plugin_wasm);

    if was_dev_created {
        // Directory was created by `dev` — clean it up entirely.
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("Failed to clean up plugin directory: {e}");
            return;
        }
        println!("Removed dev plugin '{id}' (directory cleaned up).");
        return;
    }

    // Restore the backup if it exists.
    if backup.exists() {
        if let Err(e) = std::fs::rename(&backup, &plugin_wasm) {
            eprintln!("Failed to restore plugin.wasm.bak: {e}");
            return;
        }
        // Re-cache the manifest from the restored WASM.
        if let Err(e) = cache_manifest_from_wasm(&plugin_wasm) {
            eprintln!("Warning: failed to re-cache manifest: {e}");
        }
        println!("Restored plugin.wasm from backup for '{id}'.");
    } else {
        println!("Removed dev symlink for '{id}' (no backup to restore).");
    }
}

/// Search the plugin registry.
fn handle_search(filters: DiscoveryFilters, json: bool) {
    let registry = match fetch_registry() {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("Failed to fetch plugin registry: {err}");
            return;
        }
    };

    let installed_ids: HashSet<String> = read_installed_plugins()
        .iter()
        .map(|plugin| plugin.id.clone())
        .collect();

    let matches: Vec<&MarketplaceEntry> = registry
        .plugins
        .iter()
        .filter(|plugin| matches_registry_filters(plugin, &filters, Some(&installed_ids)))
        .collect();

    if matches.is_empty() {
        println!("No plugins found.");
        return;
    }

    if json {
        let out = matches
            .iter()
            .map(|plugin| {
                serde_json::json!({
                    "id": plugin.id,
                    "name": plugin.name,
                    "version": plugin.version,
                    "summary": plugin.summary,
                    "description": plugin.description,
                    "author": plugin.author,
                    "license": plugin.license,
                    "repository": plugin.repository,
                    "categories": plugin.categories,
                    "tags": plugin.tags,
                    "capabilities": plugin.capabilities,
                    "artifact": plugin.artifact,
                    "installed": installed_ids.contains(plugin.id.as_str()),
                })
            })
            .collect::<Vec<_>>();

        match serde_json::to_string_pretty(&out) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("Failed to render JSON output: {err}"),
        }
        return;
    }

    for plugin in matches {
        let installed_suffix = if installed_ids.contains(plugin.id.as_str()) {
            " [installed]"
        } else {
            ""
        };
        println!(
            "  {:<24} v{:<10} {:<18} {}{}",
            plugin.id, plugin.version, plugin.author, plugin.summary, installed_suffix
        );
    }
}

/// Update installed plugins.
fn handle_update(specific_id: Option<&str>) {
    let registry = match fetch_registry() {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("Failed to fetch plugin registry: {err}");
            return;
        }
    };

    let installed = read_installed_plugins();
    if installed.is_empty() {
        println!("No plugins installed.");
        return;
    }

    let mut updated = 0usize;
    let mut checked = 0usize;

    for local in &installed {
        if let Some(target_id) = specific_id
            && local.id != target_id
        {
            continue;
        }

        checked += 1;

        let Some(registry_plugin) = registry.plugins.iter().find(|p| p.id == local.id) else {
            eprintln!("Skipping {}: not found in registry", local.id);
            continue;
        };

        let needs_update = local.version.as_deref() != Some(registry_plugin.version.as_str());

        if !needs_update {
            continue;
        }

        println!(
            "Updating {} {} -> {}",
            local.id,
            local.version.as_deref().unwrap_or("?"),
            registry_plugin.version
        );

        if let Err(err) = install_plugin(registry_plugin) {
            eprintln!("Failed to update {}: {err}", local.id);
            continue;
        }

        updated += 1;
    }

    if let Some(target_id) = specific_id
        && checked == 0
    {
        eprintln!("Plugin '{target_id}' is not installed.");
        return;
    }

    if updated == 0 {
        println!("All plugins are up to date.");
    } else {
        println!("Updated {updated} plugin(s).");
    }
}

/// Show details about a plugin.
fn handle_info(id: &str, json: bool) {
    let registry = fetch_registry().ok();
    let registry_plugin = registry
        .as_ref()
        .and_then(|reg| reg.plugins.iter().find(|plugin| plugin.id == id));

    let installed = read_installed_plugins();
    let installed_plugin = installed.iter().find(|plugin| plugin.id == id);

    if registry_plugin.is_none() && installed_plugin.is_none() {
        eprintln!("Plugin '{id}' was not found in registry and is not installed.");
        return;
    }

    if json {
        let out = serde_json::json!({
            "id": id,
            "registry": registry_plugin,
            "installed": installed_plugin.as_ref().map(|plugin| {
                serde_json::json!({
                    "id": plugin.id,
                    "name": plugin.name,
                    "version": plugin.version,
                    "description": plugin.description,
                    "manifestPath": plugin.manifest_path.as_ref().map(|p| p.display().to_string()),
                    "path": plugin.path.display().to_string(),
                })
            }),
        });

        match serde_json::to_string_pretty(&out) {
            Ok(text) => println!("{text}"),
            Err(err) => eprintln!("Failed to render JSON output: {err}"),
        }
        return;
    }

    if let Some(plugin) = registry_plugin {
        println!("Plugin: {} ({})", plugin.name, plugin.id);
        println!("Version: {}", plugin.version);
        println!("Summary: {}", plugin.summary);
        println!("Description: {}", plugin.description);
        println!("Author: {}", plugin.author);
        println!("License: {}", plugin.license);
        if let Some(url) = &plugin.repository {
            println!("Repository: {url}");
        }
        println!("Artifact URL: {}", plugin.artifact.url);
        println!(
            "Artifact SHA-256: {}",
            normalize_sha256(&plugin.artifact.sha256)
        );
        println!("Artifact Size: {} bytes", plugin.artifact.size);
        println!("Published At: {}", plugin.artifact.published_at);
        if !plugin.categories.is_empty() {
            println!("Categories: {}", plugin.categories.join(", "));
        }
        if !plugin.tags.is_empty() {
            println!("Tags: {}", plugin.tags.join(", "));
        }
        if !plugin.capabilities.is_empty() {
            println!("Capabilities: {}", plugin.capabilities.join(", "));
        }
        if let Some(requested) = &plugin.requested_permissions {
            let requested_text = serde_json::to_string_pretty(requested)
                .unwrap_or_else(|_| "<invalid requested_permissions>".to_string());
            println!("Requested Permissions:\n{requested_text}");
        }
    }

    if let Some(local) = installed_plugin {
        println!("Installed: yes");
        println!("Install Path: {}", local.path.display());
        if let Some(manifest_path) = &local.manifest_path {
            println!("Manifest: {}", manifest_path.display());
        }
    } else {
        println!("Installed: no");
    }
}

/// Fetch and parse the plugin registry.
fn fetch_registry() -> Result<MarketplaceRegistry, String> {
    let agent = build_http_agent(std::time::Duration::from_secs(30));
    let mut response = agent
        .get(REGISTRY_URL)
        .call()
        .map_err(|err| format!("Failed to fetch registry: {}", format_http_error(err)))?;

    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("Failed to read registry response: {err}"))?;

    MarketplaceRegistry::from_markdown(&text)
        .map_err(|err| format!("Failed to parse registry: {err}"))
}

/// Download a file and return its bytes.
fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let agent = build_http_agent(std::time::Duration::from_secs(120));
    let mut response = agent
        .get(url)
        .call()
        .map_err(|err| format!("Download failed: {}", format_http_error(err)))?;

    response
        .body_mut()
        .read_to_vec()
        .map_err(|err| format!("Failed to read response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::plugin::manifest::PluginArtifact;

    fn sample_plugin() -> MarketplaceEntry {
        MarketplaceEntry {
            id: "diaryx.sync".into(),
            name: "Sync".into(),
            version: "1.2.3".into(),
            summary: "Realtime sync".into(),
            description: "Real-time CRDT sync across devices".into(),
            author: "Diaryx Team".into(),
            license: "PolyForm Shield 1.0.0".into(),
            artifact: PluginArtifact {
                url: "https://app.diaryx.org/cdn/plugins/artifacts/diaryx.sync/1.2.3/abc.wasm"
                    .into(),
                sha256: "abc".into(),
                size: 42,
                published_at: "2026-03-03T00:00:00Z".into(),
            },
            repository: Some("https://github.com/diaryx-org/diaryx".into()),
            categories: vec!["sync".into()],
            tags: vec!["crdt".into()],
            icon: None,
            screenshots: vec![],
            capabilities: vec!["sync_transport".into()],
            requested_permissions: None,
            protocol_version: Some(1),
        }
    }

    #[test]
    fn parse_registry_md_rejects_old_schema() {
        let content = "---\nschema_version: 1\ngenerated_at: \"2026-03-03\"\nplugins: []\n---\n";
        let err = MarketplaceRegistry::from_markdown(content).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected 2"), "got: {msg}");
    }

    #[test]
    fn parse_registry_md_accepts_v2() {
        let content = r#"---
schema_version: 2
generated_at: "2026-03-03T00:00:00Z"
plugins:
  - id: "diaryx.sync"
    name: "Sync"
    version: "1.2.3"
    summary: "Realtime sync"
    description: "desc"
    author: "Diaryx Team"
    license: "MIT"
    artifact:
      url: "https://app.diaryx.org/cdn/test.wasm"
      sha256: "abc"
      size: 42
      published_at: "2026-03-03T00:00:00Z"
---
"#;
        let parsed = MarketplaceRegistry::from_markdown(content).unwrap();
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(parsed.plugins.len(), 1);
        assert_eq!(parsed.plugins[0].id, "diaryx.sync");
    }

    #[test]
    fn normalize_sha_supports_prefix() {
        assert_eq!(
            normalize_sha256("sha256:ABCDEF"),
            "abcdef",
            "prefix and case should be normalized"
        );
    }

    #[test]
    fn sha_hex_matches_known_value() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn canonical_id_validation() {
        assert!(is_canonical_plugin_id("diaryx.sync"));
        assert!(is_canonical_plugin_id("acme.plugin-1"));
        assert!(is_canonical_plugin_id("diaryx.storage.s3"));
        assert!(!is_canonical_plugin_id("sync"));
        assert!(!is_canonical_plugin_id("Diaryx.sync"));
        assert!(!is_canonical_plugin_id("diaryx..sync"));
    }

    #[test]
    fn registry_filter_matches_query_and_author() {
        let plugin = sample_plugin();
        let filters = DiscoveryFilters {
            query: Some("crdt".into()),
            author: Some("Diaryx".into()),
            ..Default::default()
        };
        let installed = HashSet::from(["diaryx.sync".to_string()]);
        assert!(matches_registry_filters(
            &plugin,
            &filters,
            Some(&installed)
        ));

        let fail_filters = DiscoveryFilters {
            author: Some("Unknown".into()),
            ..Default::default()
        };
        assert!(!matches_registry_filters(
            &plugin,
            &fail_filters,
            Some(&installed)
        ));
    }
}
