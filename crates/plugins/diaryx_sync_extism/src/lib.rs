//! Extism guest plugin for LWW file sync across devices.
//!
//! This crate compiles to a `.wasm` module loaded by the Extism host runtime.
//! It syncs workspace files to the namespace object store using hash-based
//! diffing and last-writer-wins conflict resolution.
//!
//! ## JSON exports (standard Extism protocol)
//!
//! - `manifest()` — plugin metadata + UI contributions
//! - `init()` — initialize with workspace config
//! - `shutdown()` — persist state and clean up
//! - `handle_command()` — structured commands (sync push/pull/status, etc.)
//! - `on_event()` — filesystem events from the host
//! - `get_config()` / `set_config()` — plugin configuration

mod host_fs;
#[cfg(not(target_arch = "wasm32"))]
mod native_extism_stubs;
pub mod server_api;
pub mod state;
pub mod sync_engine;
pub mod sync_manifest;

// Backend-agnostic E2E scenario bodies. Compiled only when the
// `e2e-scenarios` feature is enabled by a downstream test crate, and only on
// native targets — the wasm32 build of this plugin doesn't host tests.
#[cfg(all(feature = "e2e-scenarios", not(target_arch = "wasm32")))]
pub mod e2e_scenarios;

use diaryx_plugin_sdk::prelude::*;

use extism_pdk::*;
use serde_json::Value as JsonValue;

use diaryx_core::plugin::{SettingsField, StatusBarPosition, UiContribution};
use diaryx_plugin_sdk::protocol::ServerFunctionDecl;

#[derive(serde::Serialize, serde::Deserialize)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    write_to_disk: Option<bool>,
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct SyncExtismConfig {
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    auth_token: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}

fn normalize_server_base(server_url: &str) -> String {
    let mut base = server_url.trim().trim_end_matches('/').to_string();
    loop {
        if let Some(stripped) = base.strip_suffix("/sync2") {
            base = stripped.trim_end_matches('/').to_string();
            continue;
        }
        if let Some(stripped) = base.strip_suffix("/sync") {
            base = stripped.trim_end_matches('/').to_string();
            continue;
        }
        break;
    }
    base
}

fn load_extism_config() -> SyncExtismConfig {
    match host::storage::get("sync.extism.config") {
        Ok(Some(bytes)) => serde_json::from_slice::<SyncExtismConfig>(&bytes).unwrap_or_default(),
        _ => SyncExtismConfig::default(),
    }
}

fn save_extism_config(config: &SyncExtismConfig) {
    if let Ok(bytes) = serde_json::to_vec(config) {
        let _ = host::storage::set("sync.extism.config", &bytes);
    }
}

// ============================================================================
// Frontmatter-based config (syncs across devices)
// ============================================================================

const PLUGIN_KEY: &str = "diaryx.sync";

/// Resolve the root index file path for the current workspace.
///
/// Returns `None` if the workspace root isn't set or the root index can't be
/// found.  All errors are swallowed so callers can fall back gracefully.
fn resolve_root_index_path() -> Option<String> {
    let workspace_root = state::workspace_root()?;

    // If workspace_root is already a .md file, use it directly.
    if workspace_root.ends_with(".md") {
        return Some(workspace_root);
    }

    let dir = sync_engine::workspace_root_dir(&workspace_root);
    let fs = host_fs::HostFs;
    diaryx_core::workspace::find_root_index_in_dir_sync(&fs, std::path::Path::new(&dir))
        .ok()
        .flatten()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Read `workspace_id` from root frontmatter at `plugins."diaryx.sync".workspace_id`.
fn read_workspace_id_from_frontmatter() -> Option<String> {
    use diaryx_core::yaml_value::YamlValue;

    let root = resolve_root_index_path()?;
    let content = host::fs::read_file(&root).ok()?;
    let parsed = diaryx_core::frontmatter::parse_or_empty(&content).ok()?;

    parsed
        .frontmatter
        .get("plugins")
        .and_then(|v| v.get(PLUGIN_KEY))
        .and_then(|v| v.get("workspace_id"))
        .and_then(|v| match v {
            YamlValue::String(s) if !s.trim().is_empty() => Some(s.clone()),
            _ => None,
        })
}

/// Write `workspace_id` to root frontmatter at `plugins."diaryx.sync".workspace_id`.
/// Pass `None` to clear it.
fn write_workspace_id_to_frontmatter(workspace_id: Option<&str>) {
    use diaryx_core::yaml_value::YamlValue;
    use indexmap::IndexMap;

    let root = match resolve_root_index_path() {
        Some(r) => r,
        None => return,
    };
    let content = match host::fs::read_file(&root) {
        Ok(c) => c,
        Err(_) => return,
    };
    let parsed = match diaryx_core::frontmatter::parse_or_empty(&content) {
        Ok(p) => p,
        Err(_) => return,
    };

    let mut fm = parsed.frontmatter.clone();

    let plugins_val = fm
        .entry("plugins".to_string())
        .or_insert_with(|| YamlValue::Mapping(IndexMap::new()));
    if let Some(plugins_map) = plugins_val.as_mapping_mut() {
        let entry = plugins_map
            .entry(PLUGIN_KEY.to_string())
            .or_insert_with(|| YamlValue::Mapping(IndexMap::new()));
        if let Some(sync_map) = entry.as_mapping_mut() {
            match workspace_id {
                Some(id) => {
                    sync_map.insert(
                        "workspace_id".to_string(),
                        YamlValue::String(id.to_string()),
                    );
                }
                None => {
                    sync_map.swap_remove("workspace_id");
                    // Clean up empty maps.
                    if sync_map.is_empty() {
                        plugins_map.swap_remove(PLUGIN_KEY);
                    }
                }
            }
        }
    }

    // Clean up empty plugins map.
    if let Some(YamlValue::Mapping(m)) = fm.get("plugins") {
        if m.is_empty() {
            fm.swap_remove("plugins");
        }
    }

    if let Ok(new_content) = diaryx_core::frontmatter::serialize(&fm, &parsed.body) {
        let _ = host::fs::write_file(&root, &new_content);
    }
}

// ============================================================================
// Command parameter helpers
// ============================================================================

fn command_param_str(params: &JsonValue, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn apply_config_patch(config: &mut SyncExtismConfig, incoming: &JsonValue) {
    apply_config_string(config, incoming, "server_url", |cfg, value| {
        cfg.server_url = value
    });
    apply_config_string(config, incoming, "auth_token", |cfg, value| {
        cfg.auth_token = value
    });
    // workspace_id is written to frontmatter (syncs across devices) rather
    // than extism config.  Still update extism config for backward compat.
    if let Some(raw) = incoming.get("workspace_id") {
        if raw.is_null() || raw.as_str().is_some_and(|s| s.trim().is_empty()) {
            write_workspace_id_to_frontmatter(None);
            config.workspace_id = None;
        } else if let Some(value) = raw.as_str() {
            let trimmed = value.trim();
            write_workspace_id_to_frontmatter(Some(trimmed));
            config.workspace_id = Some(trimmed.to_string());
        }
    }
}

fn apply_config_string<F>(config: &mut SyncExtismConfig, incoming: &JsonValue, key: &str, set: F)
where
    F: FnOnce(&mut SyncExtismConfig, Option<String>),
{
    if let Some(raw) = incoming.get(key) {
        if raw.is_null() {
            set(config, None);
        } else if let Some(value) = raw.as_str() {
            let normalized = value.trim();
            if normalized.is_empty() {
                set(config, None);
            } else {
                set(config, Some(normalized.to_string()));
            }
        }
    }
}

fn resolve_server_url(params: &JsonValue, config: &SyncExtismConfig) -> Option<String> {
    command_param_str(params, "server_url")
        .or_else(|| config.server_url.clone())
        .or_else(|| runtime_context_string("server_url"))
        .map(|s| normalize_server_base(&s))
}

fn runtime_context_string(key: &str) -> Option<String> {
    host::context::get()
        .ok()
        .and_then(|runtime| {
            runtime
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .map(str::to_string)
        })
        .filter(|value| !value.is_empty())
}

fn sync_status_from_state() -> JsonValue {
    // Use the cached state value rather than re-reading frontmatter, which
    // requires file permissions that may not be configured yet.
    let has_workspace_id = state::namespace_id()
        .filter(|id| !id.trim().is_empty())
        .is_some();

    let (dirty, clean, last_sync, pending_deletes) = state::with_manifest(|m| {
        (
            m.dirty_count(),
            m.clean_count(),
            m.last_sync_at,
            m.pending_deletes.len(),
        )
    })
    .unwrap_or((0, 0, None, 0));

    let label = if !has_workspace_id {
        "Not linked"
    } else if dirty > 0 {
        "Modified"
    } else {
        "Synced"
    };

    serde_json::json!({
        "state": if dirty > 0 { "dirty" } else { "synced" },
        "label": label,
        "dirty_count": dirty,
        "clean_count": clean,
        "last_sync_at": last_sync,
        "pending_deletes": pending_deletes,
    })
}

fn get_component_html_by_id(component_id: &str) -> Option<&'static str> {
    match component_id {
        _ => None,
    }
}

fn provider_supported(params: &JsonValue) -> bool {
    command_param_str(params, "provider_id")
        .map(|id| id == "sync" || id == "diaryx.sync")
        .unwrap_or(true)
}

fn is_auth_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "401",
        "403",
        "unauthorized",
        "forbidden",
        "not authenticated",
        "authentication",
        "sign in",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn summarize_sync_errors(prefix: &str, errors: &[String]) -> String {
    const MAX_SHOWN: usize = 3;
    let shown = errors
        .iter()
        .take(MAX_SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    let suffix = if errors.len() > MAX_SHOWN {
        format!(
            " (and {} more; see plugin logs for details)",
            errors.len() - MAX_SHOWN
        )
    } else {
        String::new()
    };
    format!("{prefix}: {shown}{suffix}")
}

fn handle_get_provider_status(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Ok(serde_json::json!({
            "ready": false,
            "message": "Unsupported provider"
        }));
    }

    let config = load_extism_config();
    let has_server = resolve_server_url(params, &config)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_server {
        return Ok(serde_json::json!({
            "ready": false,
            "message": "Sync server URL is not configured"
        }));
    }

    match server_api::list_namespaces(params) {
        Ok(_) => Ok(serde_json::json!({
            "ready": true,
            "message": JsonValue::Null
        })),
        Err(error) if is_auth_error(&error) => Ok(serde_json::json!({
            "ready": false,
            "message": "Sign in to enable sync"
        })),
        Err(error) => Ok(serde_json::json!({
            "ready": false,
            "message": format!("Sync unavailable: {error}")
        })),
    }
}

fn handle_list_remote_workspaces(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Ok(serde_json::json!({ "workspaces": Vec::<JsonValue>::new() }));
    }
    let body = server_api::list_namespaces(params)?;
    let workspaces = body
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id")?.as_str()?.to_string();
            let metadata = value.get("metadata");
            let name = metadata
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or(&id)
                .to_string();
            Some(serde_json::json!({ "id": id, "name": name }))
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "workspaces": workspaces }))
}

fn handle_link_workspace(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Err("Unsupported provider".to_string());
    }
    let mut namespace_id = command_param_str(params, "namespace_id")
        .or_else(|| command_param_str(params, "remote_id"));
    let mut created = false;

    if namespace_id.is_none() {
        let name = command_param_str(params, "name").ok_or("Missing name or namespace_id")?;
        let ns = server_api::create_namespace(params, &name)?;
        namespace_id = ns.get("id").and_then(|v| v.as_str()).map(String::from);
        created = true;
    }

    let namespace_id = namespace_id.ok_or("Missing namespace_id")?;
    let workspace_root = command_param_str(params, "workspace_root")
        .or_else(|| resolve_workspace_root().ok())
        .ok_or("Missing workspace_root")?;
    let workspace_root = sync_engine::workspace_root_dir(&workspace_root);

    // Run the initial sync before persisting the workspace link. That way a
    // partial upload/download failure doesn't leave the workspace marked as
    // linked when the remote snapshot is incomplete.
    let mut manifest = sync_manifest::SyncManifest::new(namespace_id.clone());
    let result = sync_engine::sync(params, &namespace_id, &workspace_root, &mut manifest);
    if !result.errors.is_empty() {
        host::log::log(
            "warn",
            &format!("LinkWorkspace errors: {:?}", result.errors),
        );
        return Err(summarize_sync_errors(
            "LinkWorkspace initial sync failed",
            &result.errors,
        ));
    }

    write_workspace_id_to_frontmatter(Some(&namespace_id));
    state::set_namespace_id(Some(namespace_id.clone()))?;

    Ok(serde_json::json!({
        "remote_id": namespace_id,
        "created_remote": created,
        "snapshot_uploaded": result.pushed > 0,
        "sync": result,
    }))
}

fn handle_unlink_workspace(_params: &JsonValue) -> Result<JsonValue, String> {
    write_workspace_id_to_frontmatter(None);
    // Also clear extism config for backward compat.
    let mut config = load_extism_config();
    config.workspace_id = None;
    save_extism_config(&config);
    state::set_namespace_id(None)?;
    Ok(serde_json::json!({ "ok": true }))
}

fn handle_download_workspace(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Err("Unsupported provider".to_string());
    }

    let namespace_id = command_param_str(params, "remote_id")
        .or_else(|| command_param_str(params, "namespace_id"))
        .ok_or("Missing remote_id")?;

    let workspace_root =
        command_param_str(params, "workspace_root").ok_or("Missing workspace_root")?;
    let workspace_root = sync_engine::workspace_root_dir(&workspace_root);

    // Cancellation token: host UI can flip this via the cancellation registry
    // to abort the download cooperatively. Empty = no cancellation possible.
    let cancel_token = command_param_str(params, "cancel_token").unwrap_or_default();

    // Resumable: load any previously-saved manifest for this namespace. If a
    // prior DownloadWorkspace was interrupted, the Clean entries we wrote
    // will be skipped and only missing/changed files are re-pulled.
    let mut manifest = sync_manifest::SyncManifest::load(&namespace_id);
    if manifest.namespace_id != namespace_id {
        manifest = sync_manifest::SyncManifest::new(namespace_id.clone());
    }

    let result = sync_engine::execute_streaming_download(
        &namespace_id,
        &workspace_root,
        &mut manifest,
        &cancel_token,
        true, // defer non-markdown for background download
    );

    // Always persist whatever progress we made (Clean entries from successful
    // writes, PullFailed markers for write failures). On retry the next call
    // resumes where this one left off.
    manifest.save();

    if result.cancelled {
        host::log::log(
            "info",
            &format!(
                "DownloadWorkspace cancelled: {} pulled, {} resumed-skip",
                result.pulled, result.skipped_resume
            ),
        );
        return Err("DownloadWorkspace cancelled".to_string());
    }

    if !result.errors.is_empty() {
        host::log::log(
            "warn",
            &format!("DownloadWorkspace errors: {:?}", result.errors),
        );
        return Err(summarize_sync_errors(
            &format!(
                "DownloadWorkspace failed while writing {} file(s)",
                result.errors.len()
            ),
            &result.errors,
        ));
    }

    // Link the workspace if requested
    let link = params
        .get("link")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if link {
        // Save before switching state so set_namespace_id's reload picks up
        // the Clean entries we just wrote, rather than loading an empty
        // manifest and forcing every subsequent sync to re-discover them.
        manifest.save();
        write_workspace_id_to_frontmatter(Some(&namespace_id));
        state::set_namespace_id(Some(namespace_id))?;
    }

    Ok(serde_json::json!({
        "files_imported": result.pulled,
        "files_resumed_skip": result.skipped_resume,
        "deferred_files": result.deferred,
    }))
}

fn handle_upload_workspace_snapshot(params: &JsonValue) -> Result<JsonValue, String> {
    if !provider_supported(params) {
        return Err("Unsupported provider".to_string());
    }

    let namespace_id = command_param_str(params, "remote_id")
        .or_else(|| command_param_str(params, "namespace_id"))
        .or_else(|| resolve_namespace_id(params).ok())
        .ok_or("Missing remote_id")?;

    let workspace_root = command_param_str(params, "workspace_root")
        .or_else(|| resolve_workspace_root().ok())
        .ok_or("Missing workspace_root")?;
    let workspace_root = sync_engine::workspace_root_dir(&workspace_root);

    let local_scan = sync_engine::scan_local(&workspace_root, None, Some(5), Some(10));

    // Build a plan that pushes everything (empty manifest = all local files are new)
    let mut manifest = sync_manifest::SyncManifest::new(namespace_id.clone());
    let server_entries = sync_engine::fetch_server_manifest(params, &namespace_id)?;
    let plan = sync_engine::compute_diff(&manifest, &local_scan, &server_entries, &workspace_root);
    let total_ops = (plan.push.len() + plan.delete_remote.len()).max(1);

    let (pushed, _deleted_remote, errors) = sync_engine::execute_push(
        params,
        &namespace_id,
        &workspace_root,
        &plan,
        &local_scan,
        &server_entries,
        &mut manifest,
        15,
        80,
        0,
        total_ops,
    );

    if !errors.is_empty() {
        host::log::log(
            "warn",
            &format!("UploadWorkspaceSnapshot errors: {:?}", errors),
        );
        return Err(summarize_sync_errors(
            &format!(
                "UploadWorkspaceSnapshot failed while uploading {} file(s)",
                errors.len()
            ),
            &errors,
        ));
    }

    Ok(serde_json::json!({
        "files_uploaded": pushed,
        "snapshot_uploaded": pushed > 0,
    }))
}

// ---------------------------------------------------------------------------
// Sync command handlers
// ---------------------------------------------------------------------------

fn handle_sync_push(params: &JsonValue) -> Result<JsonValue, String> {
    let namespace_id = resolve_namespace_id(params)?;
    let workspace_root = resolve_workspace_root()?;

    sync_engine::emit_sync_status("syncing", None);
    let result = state::with_manifest_mut(|manifest| {
        let local_scan =
            sync_engine::scan_local(&workspace_root, Some(manifest), Some(5), Some(10));
        let server_entries = sync_engine::fetch_server_manifest(params, &namespace_id)?;
        let plan =
            sync_engine::compute_diff(manifest, &local_scan, &server_entries, &workspace_root);

        let (pushed, deleted_remote, errors) = sync_engine::execute_push(
            params,
            &namespace_id,
            &workspace_root,
            &plan,
            &local_scan,
            &server_entries,
            manifest,
            15,
            80,
            0,
            (plan.push.len() + plan.delete_remote.len()).max(1),
        );

        // Mark untracked local files as clean ONLY if the server also has
        // them (matching content, no manifest entry yet).  Local-only files
        // must stay untracked so the next sync pushes them instead of
        // treating them as "deleted on server."
        let server_key_set: std::collections::BTreeSet<&str> =
            server_entries.iter().map(|e| e.key.as_str()).collect();
        for (key, info) in &local_scan {
            if !manifest.files.contains_key(key.as_str()) && server_key_set.contains(key.as_str()) {
                manifest.mark_clean(key, &info.hash, info.size, info.modified_at);
            }
        }

        // Clean up dirty manifest entries for files no longer in the local
        // scan (deleted/renamed).  These are also absent from the server (otherwise
        // compute_diff would have placed them in plan.pull).
        let server_keys: std::collections::BTreeSet<&str> =
            server_entries.iter().map(|e| e.key.as_str()).collect();
        let orphaned: Vec<String> = manifest
            .files
            .iter()
            .filter(|(k, e)| {
                e.state == sync_manifest::SyncState::Dirty
                    && !local_scan.contains_key(k.as_str())
                    && !server_keys.contains(k.as_str())
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in &orphaned {
            manifest.files.remove(key);
        }

        manifest.save();

        if errors.is_empty() {
            sync_engine::emit_sync_status("synced", None);
        } else {
            sync_engine::emit_sync_status("error", Some(&errors.join("; ")));
        }

        Ok(serde_json::json!({
            "pushed": pushed,
            "deleted_remote": deleted_remote,
            "errors": errors,
            "orphaned_cleaned": orphaned.len(),
        }))
    })
    .unwrap_or_else(|| Err("Plugin state not initialized".to_string()))?;

    Ok(result)
}

fn handle_sync_pull(params: &JsonValue) -> Result<JsonValue, String> {
    let namespace_id = resolve_namespace_id(params)?;
    let workspace_root = resolve_workspace_root()?;

    sync_engine::emit_sync_status("syncing", None);
    let result = state::with_manifest_mut(|manifest| {
        let local_scan =
            sync_engine::scan_local(&workspace_root, Some(manifest), Some(5), Some(10));
        let server_entries = sync_engine::fetch_server_manifest(params, &namespace_id)?;
        let plan =
            sync_engine::compute_diff(manifest, &local_scan, &server_entries, &workspace_root);

        let (pulled, deleted_local, errors, deferred) = sync_engine::execute_pull(
            params,
            &namespace_id,
            &workspace_root,
            &plan,
            &server_entries,
            manifest,
            15,
            80,
            0,
            (plan.pull.len() + plan.delete_local.len()).max(1),
            true, // defer non-markdown for background download
        );

        // Mark untracked local files as clean ONLY if the server also has
        // them.  Local-only files must stay untracked so a future sync
        // pushes them instead of treating them as "deleted on server."
        let server_key_set: std::collections::BTreeSet<&str> =
            server_entries.iter().map(|e| e.key.as_str()).collect();
        for (key, info) in &local_scan {
            if !manifest.files.contains_key(key.as_str()) && server_key_set.contains(key.as_str()) {
                manifest.mark_clean(key, &info.hash, info.size, info.modified_at);
            }
        }

        manifest.save();

        if errors.is_empty() {
            sync_engine::emit_sync_status("synced", None);
        } else {
            sync_engine::emit_sync_status("error", Some(&errors.join("; ")));
        }

        Ok(serde_json::json!({
            "pulled": pulled,
            "deleted_local": deleted_local,
            "errors": errors,
            "deferred": deferred,
        }))
    })
    .unwrap_or_else(|| Err("Plugin state not initialized".to_string()))?;

    Ok(result)
}

fn handle_sync_full(params: &JsonValue) -> Result<JsonValue, String> {
    let namespace_id = resolve_namespace_id(params)?;
    let workspace_root = resolve_workspace_root()?;

    let result = state::with_manifest_mut(|manifest| {
        sync_engine::sync(params, &namespace_id, &workspace_root, manifest)
    })
    .ok_or_else(|| "Plugin state not initialized".to_string())?;

    serde_json::to_value(&result).map_err(|e| e.to_string())
}

fn handle_sync_status(_params: &JsonValue) -> Result<JsonValue, String> {
    Ok(sync_status_from_state())
}

/// Like `handle_sync_status`, but scans the filesystem first so that edits
/// made outside the running app (e.g. in a text editor or via the CLI) are
/// detected as dirty even though no `file_saved` event was received.
///
/// Used by the `SyncStatus` CLI command; the lighter `GetSyncStatus` (status
/// bar) skips the scan because the host already delivers file events.
fn handle_sync_status_with_scan(_params: &JsonValue) -> Result<JsonValue, String> {
    if let Some(workspace_root) = state::workspace_root() {
        state::with_manifest_mut(|manifest| {
            let local_scan = sync_engine::scan_local(&workspace_root, Some(manifest), None, None);
            for (key, info) in &local_scan {
                match manifest.files.get(key) {
                    Some(entry) if entry.state == sync_manifest::SyncState::Clean => {
                        if entry.content_hash != info.hash {
                            manifest.mark_dirty(key);
                        }
                    }
                    None => {
                        // New file not yet tracked
                        manifest.mark_dirty(key);
                    }
                    _ => {} // already dirty, leave it
                }
            }
        });
    }

    Ok(sync_status_from_state())
}

fn resolve_namespace_id(params: &JsonValue) -> Result<String, String> {
    command_param_str(params, "namespace_id")
        .or_else(|| read_workspace_id_from_frontmatter())
        .or_else(|| {
            // Backward compat: fall back to extism config for workspaces
            // that haven't migrated yet.
            let config = load_extism_config();
            config.workspace_id
        })
        .or_else(|| state::namespace_id())
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "No namespace linked. Use `sync link` first.".to_string())
}

fn resolve_workspace_root() -> Result<String, String> {
    state::workspace_root()
        .filter(|r| !r.trim().is_empty())
        .map(|r| sync_engine::workspace_root_dir(&r))
        .ok_or_else(|| "Missing workspace_root".to_string())
}

// ---------------------------------------------------------------------------
// Namespace API command handlers
// ---------------------------------------------------------------------------

fn handle_ns_create_namespace(params: &JsonValue) -> Result<JsonValue, String> {
    let name = command_param_str(params, "name")
        .or_else(|| command_param_str(params, "namespace_id"))
        .ok_or("Missing name")?;
    server_api::create_namespace(params, &name)
}

fn handle_ns_list_namespaces(params: &JsonValue) -> Result<JsonValue, String> {
    server_api::list_namespaces(params)
}

fn handle_ns_put_object(params: &JsonValue) -> Result<JsonValue, String> {
    let ns_id = command_param_str(params, "namespace_id").ok_or("Missing namespace_id")?;
    let key = command_param_str(params, "key").ok_or("Missing key")?;
    let content_type = command_param_str(params, "content_type")
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let body: Vec<u8> = if let Some(b64) = command_param_str(params, "body_base64") {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64)
            .map_err(|e| format!("Invalid base64: {}", e))?
    } else if let Some(text) = command_param_str(params, "body") {
        text.into_bytes()
    } else {
        return Err("Missing body or body_base64".to_string());
    };

    server_api::put_object(params, &ns_id, &key, &body, &content_type)
}

fn handle_ns_get_object(params: &JsonValue) -> Result<JsonValue, String> {
    let ns_id = command_param_str(params, "namespace_id").ok_or("Missing namespace_id")?;
    let key = command_param_str(params, "key").ok_or("Missing key")?;
    server_api::get_object(params, &ns_id, &key)
}

fn handle_ns_delete_object(params: &JsonValue) -> Result<JsonValue, String> {
    let ns_id = command_param_str(params, "namespace_id").ok_or("Missing namespace_id")?;
    let key = command_param_str(params, "key").ok_or("Missing key")?;
    server_api::delete_object(params, &ns_id, &key)?;
    Ok(JsonValue::Null)
}

fn handle_ns_list_objects(params: &JsonValue) -> Result<JsonValue, String> {
    let ns_id = command_param_str(params, "namespace_id").ok_or("Missing namespace_id")?;
    server_api::list_objects(params, &ns_id)
}

// ============================================================================
// JSON exports
// ============================================================================

fn build_manifest() -> GuestManifest {
    let sync_settings_tab = UiContribution::SettingsTab {
        id: "sync-settings".into(),
        label: "Sync".into(),
        icon: None,
        fields: vec![
            SettingsField::AuthStatus {
                label: "Account".into(),
                description: Some("Sign in to enable sync.".into()),
            },
            SettingsField::UpgradeBanner {
                feature: "Sync".into(),
                description: Some("Upgrade to sync workspaces across devices.".into()),
            },
            SettingsField::Conditional {
                condition: "plus".into(),
                fields: vec![
                    SettingsField::Section {
                        label: "Connection".into(),
                        description: None,
                    },
                    SettingsField::Text {
                        key: "server_url".into(),
                        label: "Server URL".into(),
                        description: Some("Automatically configured when you sign in.".into()),
                        placeholder: Some("https://sync.diaryx.org".into()),
                    },
                    SettingsField::Button {
                        label: "Check Status".into(),
                        command: "GetProviderStatus".into(),
                        variant: Some("outline".into()),
                    },
                ],
            },
        ],
        component: None,
    };

    let status_bar_item = UiContribution::StatusBarItem {
        id: "sync-status".into(),
        label: "Sync".into(),
        position: StatusBarPosition::Right,
        plugin_command: Some("GetSyncStatus".into()),
    };

    GuestManifest::new(
        "diaryx.sync",
        "Sync",
        env!("CARGO_PKG_VERSION"),
        "File sync across devices",
        vec![
            "workspace_events".into(),
            "file_events".into(),
            "custom_commands".into(),
        ],
    )
    .ui(vec![
        serde_json::to_value(&sync_settings_tab).unwrap_or_default(),
        serde_json::to_value(&status_bar_item).unwrap_or_default(),
        serde_json::json!({
            "slot": "WorkspaceProvider",
            "id": "diaryx.sync",
            "label": "Diaryx Sync",
            "icon": "cloud",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "sync-full",
            "label": "Sync",
            "group": "workspace",
            "plugin_command": "Sync",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "sync-pull",
            "label": "Sync Pull",
            "group": "workspace",
            "plugin_command": "SyncPull",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "sync-push",
            "label": "Sync Push",
            "group": "workspace",
            "plugin_command": "SyncPush",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "sync-status",
            "label": "Sync Status",
            "group": "workspace",
            "plugin_command": "SyncStatus",
        }),
    ])
    .commands(all_commands())
    .server_functions(vec![
        ServerFunctionDecl {
            name: "create_namespace".into(),
            method: "POST".into(),
            path: "/namespaces".into(),
            description: "Create a user-owned namespace".into(),
        },
        ServerFunctionDecl {
            name: "list_namespaces".into(),
            method: "GET".into(),
            path: "/namespaces".into(),
            description: "List namespaces owned by the authenticated user".into(),
        },
        ServerFunctionDecl {
            name: "put_object".into(),
            method: "PUT".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Store bytes under the given key in a namespace".into(),
        },
        ServerFunctionDecl {
            name: "get_object".into(),
            method: "GET".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Retrieve bytes by key from a namespace".into(),
        },
        ServerFunctionDecl {
            name: "delete_object".into(),
            method: "DELETE".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Delete an object from a namespace".into(),
        },
        ServerFunctionDecl {
            name: "list_objects".into(),
            method: "GET".into(),
            path: "/namespaces/{id}/objects".into(),
            description: "List object metadata in a namespace".into(),
        },
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "plugin_storage": { "include": ["all"], "exclude": [] },
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] },
            "delete_files": { "include": ["all"], "exclude": [] },
        }),
        reasons: [
            ("plugin_storage", "Store sync configuration and manifest"),
            ("read_files", "Read workspace files for syncing"),
            ("edit_files", "Apply remote changes to workspace files"),
            ("create_files", "Create files received from remote sync"),
            ("delete_files", "Delete files removed by remote sync"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    })
    .cli(vec![serde_json::json!({
        "name": "sync",
        "about": "Sync workspace with remote server",
        "aliases": ["sy"],
        "subcommands": [
            {
                "name": "status", "about": "Show sync status",
                "command_name": "SyncStatus"
            },
            {
                "name": "push", "about": "Push local changes to server",
                "command_name": "SyncPush"
            },
            {
                "name": "pull", "about": "Pull remote changes from server",
                "command_name": "SyncPull"
            },
            {
                "name": "link", "about": "Link this workspace to a remote namespace",
                "command_name": "LinkWorkspace",
                "args": [
                    {"name": "namespace_id", "long": "namespace-id", "help": "Existing namespace ID to link"},
                    {"name": "name", "long": "name", "help": "Create a new namespace with this name and link it"}
                ]
            },
            {
                "name": "unlink", "about": "Unlink this workspace from its remote namespace",
                "command_name": "UnlinkWorkspace"
            },
            {
                "name": "config", "about": "Configure sync settings",
                "command_name": "SyncConfig",
                "args": [
                    {"name": "server", "long": "server", "help": "Set server URL"},
                    {"name": "workspace-id", "long": "workspace-id", "help": "Set workspace ID"},
                    {"name": "show", "long": "show", "is_flag": true, "help": "Show current config"}
                ]
            }
        ]
    })])
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&build_manifest())?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or(InitParams {
        workspace_root: None,
        workspace_id: None,
        write_to_disk: None,
        server_url: None,
        auth_token: None,
    });

    // Persist server_url and auth_token to extism config (device-local).
    let mut extism_config = load_extism_config();
    if let Some(server_url) = &params.server_url {
        extism_config.server_url = Some(server_url.clone());
    }
    if let Some(auth_token) = &params.auth_token {
        extism_config.auth_token = Some(auth_token.clone());
    }
    save_extism_config(&extism_config);

    // Initialize state with workspace_root first so frontmatter helpers work.
    // Resolve workspace_id: params > frontmatter > extism config (backward compat).
    state::init_state(None, params.workspace_root.clone());

    let fm_workspace_id = read_workspace_id_from_frontmatter();

    let workspace_id = params
        .workspace_id
        .clone()
        .or(fm_workspace_id)
        .or(extism_config.workspace_id.clone());

    state::init_state(workspace_id, params.workspace_root.clone());

    host::log::log("info", "Sync plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    state::shutdown_state();
    host::log::log("info", "Sync plugin shut down");
    Ok(String::new())
}

fn command_response(result: Result<JsonValue, String>) -> CommandResponse {
    match result {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
    }
}

fn execute_command(req: CommandRequest) -> CommandResponse {
    let CommandRequest { command, params } = req;

    let result: Option<Result<JsonValue, String>> = match command.as_str() {
        "get_component_html" => {
            let component_id = params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            Some(
                get_component_html_by_id(component_id)
                    .map(|html| JsonValue::String(html.to_string()))
                    .ok_or_else(|| format!("Unknown sync component: {component_id}")),
            )
        }
        "get_config" => Some(Ok(
            serde_json::to_value(load_extism_config()).unwrap_or_default()
        )),
        "set_config" => {
            let mut config = load_extism_config();
            apply_config_patch(&mut config, &params);
            save_extism_config(&config);
            Some(Ok(JsonValue::Null))
        }
        "GetSyncStatus" => Some(handle_sync_status(&params)),
        "GetProviderStatus" => Some(handle_get_provider_status(&params)),
        "ListRemoteWorkspaces" => Some(handle_list_remote_workspaces(&params)),
        "LinkWorkspace" => Some(handle_link_workspace(&params)),
        "UnlinkWorkspace" => Some(handle_unlink_workspace(&params)),
        "DownloadWorkspace" => Some(handle_download_workspace(&params)),
        "UploadWorkspaceSnapshot" => Some(handle_upload_workspace_snapshot(&params)),
        // Sync commands
        "SyncPush" | "sync_push" => Some(handle_sync_push(&params)),
        "SyncPull" | "sync_pull" => Some(handle_sync_pull(&params)),
        "Sync" | "sync" => Some(handle_sync_full(&params)),
        "SyncStatus" | "sync_status" => Some(handle_sync_status_with_scan(&params)),
        // Namespace API commands
        "NsCreateNamespace" => Some(handle_ns_create_namespace(&params)),
        "NsListNamespaces" => Some(handle_ns_list_namespaces(&params)),
        "NsPutObject" => Some(handle_ns_put_object(&params)),
        "NsGetObject" => Some(handle_ns_get_object(&params)),
        "NsDeleteObject" => Some(handle_ns_delete_object(&params)),
        "NsListObjects" => Some(handle_ns_list_objects(&params)),
        _ => None,
    };

    if let Some(result) = result {
        return command_response(result);
    }

    CommandResponse::err(format!("Unknown command: {command}"))
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = execute_command(req);
    Ok(serde_json::to_string(&response)?)
}

/// Handle a filesystem/workspace event from the host.
#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: GuestEvent = serde_json::from_str(&input)?;

    match event.event_type.as_str() {
        "file_saved" | "file_created" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = workspace_relative_path(path);
                state::with_manifest_mut(|m| {
                    m.mark_dirty(&format!("files/{relative}"));
                    m.save();
                });
            }
        }
        "file_deleted" => {
            if let Some(path) = event.payload.get("path").and_then(|v| v.as_str()) {
                let relative = workspace_relative_path(path);
                state::with_manifest_mut(|m| {
                    m.record_delete(&format!("files/{relative}"));
                    m.save();
                });
            }
        }
        "file_renamed" | "file_moved" => {
            let old_path = event.payload.get("old_path").and_then(|v| v.as_str());
            let new_path = event.payload.get("new_path").and_then(|v| v.as_str());
            if let (Some(old), Some(new)) = (old_path, new_path) {
                let old_relative = workspace_relative_path(old);
                let new_relative = workspace_relative_path(new);
                state::with_manifest_mut(|m| {
                    m.record_delete(&format!("files/{old_relative}"));
                    m.mark_dirty(&format!("files/{new_relative}"));
                    m.save();
                });
            }
        }
        _ => {}
    }

    Ok(String::new())
}

/// Get plugin configuration.
#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&load_extism_config())?)
}

/// Set plugin configuration.
#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let incoming: JsonValue = serde_json::from_str(&input)?;
    let mut config = load_extism_config();
    apply_config_patch(&mut config, &incoming);
    save_extism_config(&config);
    Ok(String::new())
}

/// Execute a typed Command.
#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    let parsed: JsonValue = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid JSON: {e}")))?;

    let cmd_type = parsed["type"]
        .as_str()
        .ok_or_else(|| extism_pdk::Error::msg("Missing 'type' field in command"))?;

    let params = parsed.get("params").cloned().unwrap_or(JsonValue::Null);

    let resp = execute_command(CommandRequest {
        command: cmd_type.to_string(),
        params,
    });

    if resp.success {
        let response = serde_json::json!({ "type": "PluginResult", "data": resp.data });
        Ok(serde_json::to_string(&response)?)
    } else if let Some(ref error) = resp.error {
        if error.starts_with("Unknown command:") {
            Ok(String::new())
        } else {
            Err(extism_pdk::Error::msg(error.clone()).into())
        }
    } else {
        Ok(String::new())
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn workspace_relative_path(path: &str) -> String {
    let normalized_path = path.replace('\\', "/");
    let root = state::workspace_root().unwrap_or_default();
    let root = root.trim();
    if root.is_empty() || root == "." {
        return normalized_path;
    }
    let normalized_root = sync_engine::workspace_root_dir(root).replace('\\', "/");
    let normalized_root = normalized_root.trim_end_matches('/');

    if let Some(stripped) = normalized_path.strip_prefix(&format!("{normalized_root}/")) {
        stripped.to_string()
    } else {
        normalized_path
    }
}

fn all_commands() -> Vec<String> {
    vec![
        // Sync
        "SyncPush",
        "SyncPull",
        "Sync",
        "SyncStatus",
        "GetSyncStatus",
        // Provider
        "GetProviderStatus",
        "ListRemoteWorkspaces",
        "LinkWorkspace",
        "UnlinkWorkspace",
        "DownloadWorkspace",
        "UploadWorkspaceSnapshot",
        // Iframe Components
        "get_component_html",
        "get_config",
        "set_config",
        // Namespace API
        "NsCreateNamespace",
        "NsListNamespaces",
        "NsPutObject",
        "NsGetObject",
        "NsDeleteObject",
        "NsListObjects",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_settings_tab_uses_declarative_fields() {
        let manifest = build_manifest();
        let tab = manifest
            .ui
            .iter()
            .find(|ui| {
                ui.get("slot").and_then(|v| v.as_str()) == Some("SettingsTab")
                    && ui.get("id").and_then(|v| v.as_str()) == Some("sync-settings")
            })
            .expect("sync settings tab should exist");

        assert!(
            tab.get("component").unwrap().is_null(),
            "settings tab component should be null (no iframe)"
        );

        let fields = tab
            .get("fields")
            .and_then(|v| v.as_array())
            .expect("fields should be an array");
        assert!(!fields.is_empty(), "fields should not be empty");

        assert_eq!(
            fields[0].get("type").and_then(|v| v.as_str()),
            Some("AuthStatus")
        );

        assert_eq!(
            fields[1].get("type").and_then(|v| v.as_str()),
            Some("UpgradeBanner")
        );

        assert_eq!(
            fields[2].get("type").and_then(|v| v.as_str()),
            Some("Conditional")
        );
    }

    #[test]
    fn manifest_declares_requested_permissions() {
        let manifest = build_manifest();
        let perms = manifest
            .requested_permissions
            .as_ref()
            .expect("manifest should declare requested_permissions");

        assert!(perms.defaults.get("plugin_storage").is_some());
        assert!(perms.defaults.get("http_requests").is_none());
    }

    #[test]
    fn apply_config_patch_clears_and_sets_values() {
        let mut cfg = SyncExtismConfig {
            server_url: Some("https://old.example".to_string()),
            auth_token: Some("old-token".to_string()),
            workspace_id: Some("old-workspace".to_string()),
        };

        let patch = serde_json::json!({
            "server_url": null,
            "auth_token": "  ",
            "workspace_id": "new-workspace"
        });
        apply_config_patch(&mut cfg, &patch);

        assert_eq!(cfg.server_url, None);
        assert_eq!(cfg.auth_token, None);
        assert_eq!(cfg.workspace_id.as_deref(), Some("new-workspace"));
    }

    #[test]
    fn normalize_server_base_strips_sync_suffixes_and_trailing_slashes() {
        assert_eq!(
            normalize_server_base("https://sync.diaryx.org/sync2/"),
            "https://sync.diaryx.org"
        );
        assert_eq!(
            normalize_server_base("https://sync.diaryx.org/sync/"),
            "https://sync.diaryx.org"
        );
    }

    #[test]
    fn workspace_relative_path_strips_root() {
        // Simulate state not initialized — falls back to returning path as-is
        let result = workspace_relative_path("/workspace/doc.md");
        assert_eq!(result, "/workspace/doc.md");
    }

    #[test]
    fn workspace_relative_path_uses_workspace_directory_when_root_is_index_file() {
        state::init_state(None, Some("/workspace/index.md".to_string()));
        let result = workspace_relative_path("/workspace/docs/doc.md");
        assert_eq!(result, "docs/doc.md");
        state::shutdown_state();
    }

    #[test]
    fn cli_has_push_pull_no_start() {
        let manifest = build_manifest();
        let cli = &manifest.cli;
        let sync_cmd = cli[0].as_object().unwrap();
        let subcommands = sync_cmd["subcommands"].as_array().unwrap();
        let names: Vec<&str> = subcommands
            .iter()
            .filter_map(|s: &JsonValue| s.get("name").and_then(|v| v.as_str()))
            .collect();
        assert!(names.contains(&"push"));
        assert!(names.contains(&"pull"));
        assert!(!names.contains(&"start"));
    }
}
