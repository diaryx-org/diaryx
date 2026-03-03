//! Plugin management commands — install, remove, list, search, update, info.
//!
//! Downloads plugins from the Diaryx CDN `registry-v2.json` and manages the
//! local plugin directory at `~/.diaryx/plugins/`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_extism::{HostContext, inspect_plugin_wasm_manifest, load_plugin_from_wasm};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli::args::PluginCommands;

const REGISTRY_URL: &str = "https://cdn.diaryx.org/plugins/registry-v2.json";

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryV2 {
    schema_version: u64,
    generated_at: String,
    plugins: Vec<RegistryPlugin>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryPlugin {
    id: String,
    name: String,
    version: String,
    summary: String,
    description: String,
    creator: String,
    license: String,
    artifact: RegistryArtifact,
    source: RegistrySource,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    documentation_url: Option<String>,
    #[serde(default)]
    changelog_url: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    screenshots: Vec<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    requested_permissions: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryArtifact {
    wasm_url: String,
    sha256: String,
    size_bytes: u64,
    published_at: String,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistrySource {
    kind: String,
    repository_url: String,
    registry_id: String,
}

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
    source: Option<String>,
    creator: Option<String>,
    installed_only: bool,
}

/// Handle the `diaryx plugin <subcommand>` family.
pub fn handle_plugin_command(command: PluginCommands) {
    match command {
        PluginCommands::List {
            category,
            tag,
            source,
            creator,
            json,
        } => handle_list(
            DiscoveryFilters {
                category,
                tag,
                source,
                creator,
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
            source,
            creator,
            installed,
            json,
        } => handle_search(
            DiscoveryFilters {
                query,
                category,
                tag,
                source,
                creator,
                installed_only: installed,
            },
            json,
        ),
        PluginCommands::Update { id } => handle_update(id.as_deref()),
        PluginCommands::Info { id, json } => handle_info(&id, json),
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

fn read_manifest_json(path: &Path) -> Option<serde_json::Value> {
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&json).ok()
}

fn read_installed_plugins() -> Vec<InstalledPlugin> {
    let dir = plugins_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut installed = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
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

    installed.sort_by(|a, b| a.id.cmp(&b.id));
    installed
}

fn normalize_optional_filter(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
}

fn normalized_source_filter(raw: &Option<String>) -> Result<Option<String>, String> {
    let source = normalize_optional_filter(raw);
    if let Some(source_kind) = &source
        && source_kind != "internal"
        && source_kind != "external"
    {
        return Err(format!(
            "Invalid --source value '{source_kind}'. Expected 'internal' or 'external'."
        ));
    }
    Ok(source)
}

fn matches_registry_filters(
    plugin: &RegistryPlugin,
    filters: &DiscoveryFilters,
    installed_ids: Option<&HashSet<String>>,
) -> bool {
    let query = normalize_optional_filter(&filters.query);
    let category = normalize_optional_filter(&filters.category);
    let tag = normalize_optional_filter(&filters.tag);
    let creator = normalize_optional_filter(&filters.creator);
    let source = match normalized_source_filter(&filters.source) {
        Ok(value) => value,
        Err(_) => return false,
    };

    if let Some(q) = query {
        let haystack = format!(
            "{} {} {} {} {} {} {}",
            plugin.id,
            plugin.name,
            plugin.summary,
            plugin.description,
            plugin.creator,
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

    if let Some(expected_source) = source
        && !plugin.source.kind.eq_ignore_ascii_case(&expected_source)
    {
        return false;
    }

    if let Some(expected_creator) = creator
        && !plugin
            .creator
            .to_lowercase()
            .contains(expected_creator.as_str())
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

/// List installed plugins.
fn handle_list(filters: DiscoveryFilters, json: bool) {
    if let Err(err) = normalized_source_filter(&filters.source) {
        eprintln!("{err}");
        return;
    }

    let installed = read_installed_plugins();
    if installed.is_empty() {
        println!("No plugins installed.");
        return;
    }

    let registry = fetch_registry().ok();
    let registry_by_id: HashMap<String, RegistryPlugin> = registry
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
        || filters.source.is_some()
        || filters.creator.is_some()
        || filters.query.is_some();

    if using_metadata_filter && registry.is_none() {
        eprintln!("Could not fetch registry-v2 metadata. Retry later or remove metadata filters.");
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
                    "source": registry_plugin.as_ref().map(|p| p.source.kind.clone()),
                    "creator": registry_plugin.as_ref().map(|p| p.creator.clone()),
                    "license": registry_plugin.as_ref().map(|p| p.license.clone()),
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
                "  {:<24} v{:<10} {:<11} {:<18} {}",
                local.id,
                version,
                registry_plugin.source.kind,
                registry_plugin.creator,
                registry_plugin.summary
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
fn install_plugin(plugin: &RegistryPlugin) -> Result<(), String> {
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

        let bytes = download_bytes(&plugin.artifact.wasm_url)?;
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

fn verify_download_integrity(plugin: &RegistryPlugin, bytes: &[u8]) -> Result<(), String> {
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

    if plugin.artifact.size_bytes != bytes.len() as u64 {
        return Err(format!(
            "Artifact size mismatch for {}: expected {} bytes, got {} bytes",
            plugin.id,
            plugin.artifact.size_bytes,
            bytes.len()
        ));
    }

    Ok(())
}

fn verify_inspected_manifest(
    registry_plugin: &RegistryPlugin,
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

    if inspected.description != registry_plugin.description {
        return Err(format!(
            "Manifest description mismatch for {}",
            registry_plugin.id
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
    let legacy = plugins_dir().join(id);
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

/// Search the plugin registry.
fn handle_search(filters: DiscoveryFilters, json: bool) {
    if let Err(err) = normalized_source_filter(&filters.source) {
        eprintln!("{err}");
        return;
    }

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

    let matches: Vec<&RegistryPlugin> = registry
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
                    "creator": plugin.creator,
                    "license": plugin.license,
                    "source": plugin.source,
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
            "  {:<24} v{:<10} {:<11} {:<18} {}{}",
            plugin.id,
            plugin.version,
            plugin.source.kind,
            plugin.creator,
            plugin.summary,
            installed_suffix
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
            eprintln!("Skipping {}: not found in registry-v2", local.id);
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
        eprintln!("Plugin '{id}' was not found in registry-v2 and is not installed.");
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
        println!("Creator: {}", plugin.creator);
        println!("License: {}", plugin.license);
        println!(
            "Source: {} ({})",
            plugin.source.kind, plugin.source.registry_id
        );
        println!("Repository: {}", plugin.source.repository_url);
        println!("Artifact URL: {}", plugin.artifact.wasm_url);
        println!(
            "Artifact SHA-256: {}",
            normalize_sha256(&plugin.artifact.sha256)
        );
        println!("Artifact Size: {} bytes", plugin.artifact.size_bytes);
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
        if let Some(url) = &plugin.homepage {
            println!("Homepage: {url}");
        }
        if let Some(url) = &plugin.documentation_url {
            println!("Documentation: {url}");
        }
        if let Some(url) = &plugin.changelog_url {
            println!("Changelog: {url}");
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

fn parse_registry_payload(payload: serde_json::Value) -> Result<RegistryV2, String> {
    let Some(schema_version) = payload.get("schemaVersion").and_then(|v| v.as_u64()) else {
        return Err(
            "Unsupported plugin registry schema version: missing schemaVersion (expected 2)"
                .to_string(),
        );
    };

    if schema_version != 2 {
        return Err(format!(
            "Unsupported plugin registry schema version: {} (expected 2)",
            schema_version
        ));
    }

    let registry = serde_json::from_value::<RegistryV2>(payload)
        .map_err(|err| format!("Failed to parse registry-v2 payload: {err}"))?;

    if registry.schema_version != 2 {
        return Err(format!(
            "Unsupported plugin registry schema version: {} (expected 2)",
            registry.schema_version
        ));
    }

    for plugin in &registry.plugins {
        validate_registry_plugin(plugin)?;
    }

    Ok(registry)
}

fn validate_registry_plugin(plugin: &RegistryPlugin) -> Result<(), String> {
    if plugin.id.trim().is_empty() {
        return Err("registry-v2 validation error: plugin.id must be non-empty".to_string());
    }
    if !is_canonical_plugin_id(plugin.id.as_str()) {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has non-canonical id",
            plugin.id
        ));
    }
    if plugin.name.trim().is_empty() {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has empty name",
            plugin.id
        ));
    }
    if plugin.version.trim().is_empty() {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has empty version",
            plugin.id
        ));
    }
    if plugin.artifact.wasm_url.trim().is_empty() {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has empty artifact.wasmUrl",
            plugin.id
        ));
    }
    if normalize_sha256(&plugin.artifact.sha256).is_empty() {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has empty artifact.sha256",
            plugin.id
        ));
    }
    if plugin.artifact.size_bytes == 0 {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has invalid artifact.sizeBytes=0",
            plugin.id
        ));
    }
    if plugin.source.kind != "internal" && plugin.source.kind != "external" {
        return Err(format!(
            "registry-v2 validation error: plugin '{}' has invalid source.kind '{}'",
            plugin.id, plugin.source.kind
        ));
    }
    Ok(())
}

/// Fetch and parse the plugin registry.
fn fetch_registry() -> Result<RegistryV2, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|err| format!("HTTP client error: {err}"))?;

    let response = client
        .get(REGISTRY_URL)
        .send()
        .map_err(|err| format!("Failed to fetch registry-v2: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("Registry returned status {}", response.status()));
    }

    let payload = response
        .json::<serde_json::Value>()
        .map_err(|err| format!("Failed to decode registry payload: {err}"))?;

    parse_registry_payload(payload)
}

/// Download a file and return its bytes.
fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|err| format!("HTTP client error: {err}"))?;

    let response = client
        .get(url)
        .send()
        .map_err(|err| format!("Download failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("Download returned status {}", response.status()));
    }

    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|err| format!("Failed to read response: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plugin() -> RegistryPlugin {
        RegistryPlugin {
            id: "diaryx.sync".into(),
            name: "Sync".into(),
            version: "1.2.3".into(),
            summary: "Realtime sync".into(),
            description: "Real-time CRDT sync across devices".into(),
            creator: "Diaryx Team".into(),
            license: "PolyForm Shield 1.0.0".into(),
            artifact: RegistryArtifact {
                wasm_url: "https://cdn.diaryx.org/plugins/artifacts/diaryx.sync/1.2.3/abc.wasm"
                    .into(),
                sha256: "abc".into(),
                size_bytes: 42,
                published_at: "2026-03-03T00:00:00Z".into(),
            },
            source: RegistrySource {
                kind: "internal".into(),
                repository_url: "https://github.com/diaryx-org/diaryx".into(),
                registry_id: "diaryx-official".into(),
            },
            homepage: None,
            documentation_url: None,
            changelog_url: None,
            categories: vec!["sync".into()],
            tags: vec!["crdt".into()],
            icon_url: None,
            screenshots: vec![],
            capabilities: vec!["sync_transport".into()],
            requested_permissions: None,
        }
    }

    #[test]
    fn parse_registry_rejects_old_schema() {
        let payload = serde_json::json!({
            "schemaVersion": 1,
            "generatedAt": "2026-03-03T00:00:00Z",
            "plugins": []
        });
        let err = parse_registry_payload(payload).expect_err("schema 1 should fail");
        assert!(err.contains("expected 2"));
    }

    #[test]
    fn parse_registry_accepts_v2() {
        let payload = serde_json::json!({
            "schemaVersion": 2,
            "generatedAt": "2026-03-03T00:00:00Z",
            "plugins": [sample_plugin()]
        });
        let parsed = parse_registry_payload(payload).expect("schema 2 should parse");
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
    fn registry_filter_matches_query_and_source() {
        let plugin = sample_plugin();
        let filters = DiscoveryFilters {
            query: Some("crdt".into()),
            source: Some("internal".into()),
            ..Default::default()
        };
        let installed = HashSet::from(["diaryx.sync".to_string()]);
        assert!(matches_registry_filters(
            &plugin,
            &filters,
            Some(&installed)
        ));

        let fail_filters = DiscoveryFilters {
            source: Some("external".into()),
            ..Default::default()
        };
        assert!(!matches_registry_filters(
            &plugin,
            &fail_filters,
            Some(&installed)
        ));
    }
}
