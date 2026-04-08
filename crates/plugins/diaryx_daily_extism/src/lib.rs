//! Extism guest plugin for Diaryx daily entry functionality.

mod daily_logic;

use diaryx_plugin_sdk::prelude::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use daily_logic::{
    DailyDirection, DailyPluginConfig, adjacent_daily_entry_path, date_from_filename,
    default_entry_template, normalize_folder, parse_date_input, parse_rfc3339_date_in_offset,
    path_to_date, paths_for_date, render_template,
};
use diaryx_core::frontmatter;
use diaryx_core::link_parser::{
    LinkFormat, format_link_with_format, parse_link, to_canonical_with_link_format,
};
use diaryx_core::yaml_value::YamlValue;
use extism_pdk::*;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct DailyState {
    workspace_root: Option<String>,
    config: DailyPluginConfig,
    link_format: LinkFormat,
}

// WASM is single-threaded; use RefCell instead of Mutex to avoid panics on
// re-entrant host function calls (host may dispatch events while a host_*
// call is in flight, leading to recursive lock attempts).
thread_local! {
    static STATE: RefCell<DailyState> = RefCell::new(DailyState::default());
}

fn current_state() -> Result<DailyState, String> {
    STATE.with(|cell| Ok(cell.borrow().clone()))
}

fn with_state_mut<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut DailyState) -> Result<R, String>,
{
    STATE.with(|cell| {
        let mut state = cell.borrow().clone();
        let result = f(&mut state)?;
        *cell.borrow_mut() = state;
        Ok(result)
    })
}

fn current_local_datetime() -> Result<DateTime<FixedOffset>, String> {
    let raw = host::time::now_rfc3339()?;
    DateTime::parse_from_rfc3339(raw.trim())
        .map_err(|e| format!("failed to parse host_get_now response: {e}"))
}

fn current_local_date() -> Result<NaiveDate, String> {
    Ok(current_local_datetime()?.date_naive())
}

fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn is_absolute_path(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    path.len() > 1 && path.as_bytes()[1] == b':'
}

fn to_fs_path(rel_path: &str, workspace_root: Option<&str>) -> String {
    let rel = normalize_rel_path(rel_path);
    match workspace_root {
        Some(root) if !root.trim().is_empty() => {
            if root.ends_with(".md")
                && let Some(parent) = Path::new(root).parent()
            {
                return parent.join(rel).to_string_lossy().to_string();
            }
            if is_absolute_path(root) {
                return Path::new(root).join(rel).to_string_lossy().to_string();
            }
            if root == "." {
                rel
            } else {
                Path::new(root).join(rel).to_string_lossy().to_string()
            }
        }
        _ => rel,
    }
}

fn to_workspace_rel(path: &str, workspace_root: Option<&str>) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(root) = workspace_root
        && is_absolute_path(root)
    {
        let root_path = Path::new(root);
        let input_path = Path::new(path);
        if input_path.is_absolute()
            && let Ok(stripped) = input_path.strip_prefix(root_path)
        {
            return normalize_rel_path(&stripped.to_string_lossy());
        }
    }
    normalize_rel_path(&normalized)
}

fn storage_key_for_workspace(workspace_root: Option<&str>) -> String {
    let token = workspace_root.unwrap_or("__default__");
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    format!("daily.config.{:x}", hasher.finish())
}

fn load_workspace_config(workspace_root: Option<&str>) -> DailyPluginConfig {
    let key = storage_key_for_workspace(workspace_root);
    match host::storage::get(&key) {
        Ok(Some(bytes)) => serde_json::from_slice::<DailyPluginConfig>(&bytes).unwrap_or_default(),
        _ => DailyPluginConfig::default(),
    }
}

fn save_workspace_config(state: &DailyState) -> Result<(), String> {
    let key = storage_key_for_workspace(state.workspace_root.as_deref());
    let bytes = serde_json::to_vec(&state.config).map_err(|e| format!("serialize config: {e}"))?;
    host::storage::set(&key, &bytes)?;
    Ok(())
}

fn find_root_index_candidates(workspace_root: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = workspace_root {
        if root.ends_with(".md") {
            out.push(root.to_string());
        } else {
            out.push(to_fs_path("README.md", Some(root)));
        }
    }
    out.push("README.md".to_string());
    out
}

fn parse_markdown(content: &str) -> Result<(IndexMap<String, YamlValue>, String), String> {
    let parsed = frontmatter::parse_or_empty(content).map_err(|e| e.to_string())?;
    Ok((parsed.frontmatter, parsed.body))
}

fn write_markdown(
    fs_path: &str,
    frontmatter_map: &IndexMap<String, YamlValue>,
    body: &str,
) -> Result<(), String> {
    let serialized = frontmatter::serialize(frontmatter_map, body).map_err(|e| e.to_string())?;
    host::fs::write_file(fs_path, &serialized)
}

fn parse_link_format_str(s: &str) -> Option<LinkFormat> {
    match s {
        "markdown_root" => Some(LinkFormat::MarkdownRoot),
        "markdown_relative" => Some(LinkFormat::MarkdownRelative),
        "plain_relative" => Some(LinkFormat::PlainRelative),
        "plain_canonical" => Some(LinkFormat::PlainCanonical),
        _ => None,
    }
}

fn read_link_format(state: &DailyState) -> LinkFormat {
    let root_rel = match find_existing_root_index_rel(state) {
        Ok(Some(rel)) => rel,
        _ => return LinkFormat::default(),
    };
    let fs_path = to_fs_path(&root_rel, state.workspace_root.as_deref());
    let content = match host::fs::read_file(&fs_path) {
        Ok(c) => c,
        Err(_) => return LinkFormat::default(),
    };
    let (fm, _) = match parse_markdown(&content) {
        Ok(v) => v,
        Err(_) => return LinkFormat::default(),
    };

    // Check workspace_config mapping or file link
    match fm.get("workspace_config") {
        Some(YamlValue::Mapping(config_map)) => {
            if let Some(YamlValue::String(s)) = config_map.get("link_format") {
                if let Some(fmt) = parse_link_format_str(s) {
                    return fmt;
                }
            }
        }
        Some(YamlValue::String(link_str)) => {
            // File link to workspace config (e.g., "[Config](/Meta/Config.md)")
            let config_rel = resolve_link_path(link_str, &root_rel);
            let config_fs = to_fs_path(&config_rel, state.workspace_root.as_deref());
            if let Ok(config_content) = host::fs::read_file(&config_fs) {
                if let Ok((config_fm, _)) = parse_markdown(&config_content) {
                    if let Some(YamlValue::String(s)) = config_fm.get("link_format") {
                        if let Some(fmt) = parse_link_format_str(s) {
                            return fmt;
                        }
                    }
                }
            }
        }
        _ => {}
    }

    // Fall back to top-level link_format
    if let Some(YamlValue::String(s)) = fm.get("link_format") {
        if let Some(fmt) = parse_link_format_str(s) {
            return fmt;
        }
    }

    LinkFormat::default()
}

fn format_link_for(from_rel: &str, to_rel: &str, title: &str, format: LinkFormat) -> String {
    format_link_with_format(to_rel, title, format, from_rel)
}

fn read_title_from_file(state: &DailyState, rel_path: &str) -> Option<String> {
    let fs_path = to_fs_path(rel_path, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path).ok()?;
    let (fm, _) = parse_markdown(&content).ok()?;
    fm.get("title")
        .and_then(YamlValue::as_str)
        .map(|s| s.to_string())
}

fn ensure_sequence(frontmatter_map: &mut IndexMap<String, YamlValue>, key: &str) -> Vec<String> {
    match frontmatter_map.get(key) {
        Some(YamlValue::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn save_sequence(frontmatter_map: &mut IndexMap<String, YamlValue>, key: &str, values: &[String]) {
    let seq = values
        .iter()
        .map(|v| YamlValue::String(v.clone()))
        .collect::<Vec<_>>();
    frontmatter_map.insert(key.to_string(), YamlValue::Sequence(seq));
}

fn root_index_scope(path: Option<&str>) -> String {
    let Some(path) = path.map(str::trim).filter(|value| !value.is_empty()) else {
        return "README.md".to_string();
    };

    let normalized = if is_absolute_path(path) {
        Path::new(path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "README.md".to_string())
    } else {
        normalize_rel_path(path)
    };

    if normalized.is_empty() {
        "README.md".to_string()
    } else {
        normalized
    }
}

fn unique_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for scope in scopes {
        if !scope.is_empty() && !out.iter().any(|existing| existing == &scope) {
            out.push(scope);
        }
    }
    out
}

fn requested_permissions_for(folder: &str, root_index_path: Option<&str>) -> JsonValue {
    let folder_scope = normalize_rel_path(folder);
    let root_scope = root_index_scope(root_index_path);
    let read_edit_scopes = unique_scopes(vec![folder_scope.clone(), root_scope]);

    serde_json::json!({
        "defaults": {
            "read_files": { "include": read_edit_scopes.clone(), "exclude": [] },
            "edit_files": { "include": read_edit_scopes, "exclude": [] },
            "create_files": { "include": [folder_scope], "exclude": [] },
            "plugin_storage": { "include": ["all"], "exclude": [] }
        },
        "reasons": {
            "read_files": "Read daily entries, index files, and optional templates from the workspace.",
            "edit_files": "Update the root index plus year, month, and daily entry files when organizing the daily hierarchy.",
            "create_files": "Create missing year, month, and daily entry files for new dates.",
            "plugin_storage": "Persist daily plugin configuration for the current workspace."
        }
    })
}

fn build_requested_permissions(
    folder: &str,
    root_index_path: Option<&str>,
) -> GuestRequestedPermissions {
    let perms = requested_permissions_for(folder, root_index_path);
    let defaults = perms.get("defaults").cloned().unwrap_or(JsonValue::Null);
    let reasons_value = perms.get("reasons").cloned().unwrap_or(JsonValue::Null);
    let reasons = if let Some(obj) = reasons_value.as_object() {
        obj.iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect::<HashMap<String, String>>()
    } else {
        HashMap::new()
    };
    GuestRequestedPermissions { defaults, reasons }
}

fn build_permissions_patch(folder: &str, root_index_path: Option<&str>) -> JsonValue {
    let defaults = requested_permissions_for(folder, root_index_path)
        .get("defaults")
        .cloned()
        .unwrap_or(JsonValue::Null);

    serde_json::json!({
        "plugin_permissions_patch": {
            "plugin_id": "diaryx.daily",
            "mode": "replace",
            "permissions": defaults
        }
    })
}

fn root_index_rel_candidates(state: &DailyState) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(root) = state.workspace_root.as_deref()
        && root.ends_with(".md")
    {
        out.push(root_index_scope(Some(root)));
    }

    if !out.iter().any(|value| value == "README.md") {
        out.push("README.md".to_string());
    }

    out
}

fn find_existing_root_index_rel(state: &DailyState) -> Result<Option<String>, String> {
    for candidate in root_index_rel_candidates(state) {
        let fs_path = to_fs_path(&candidate, state.workspace_root.as_deref());
        if host::fs::file_exists(&fs_path)? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn extract_entry_folder_update(params: &JsonValue) -> Option<Option<String>> {
    if params.get("source").and_then(|value| value.as_str()) == Some("workspace_config") {
        if params.get("field").and_then(|value| value.as_str()) != Some("daily_entry_folder") {
            return None;
        }

        let raw_value = params
            .get("value")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let normalized = normalize_folder(Some(raw_value));
        return Some((!normalized.is_empty()).then_some(normalized));
    }

    let config = params.get("config")?.as_object()?;
    let value = config.get("entry_folder")?;
    if value.is_null() {
        return Some(None);
    }

    let normalized = normalize_folder(value.as_str());
    Some((!normalized.is_empty()).then_some(normalized))
}

fn handle_update_config(params: JsonValue) -> Result<JsonValue, String> {
    with_state_mut(|state| {
        if let Some(next_entry_folder) = extract_entry_folder_update(&params) {
            state.config.entry_folder = next_entry_folder;
            save_workspace_config(state)?;
        }

        let folder = state.config.effective_entry_folder();
        let root_index_path = params
            .get("root_index_path")
            .and_then(|value| value.as_str())
            .or_else(|| {
                state
                    .workspace_root
                    .as_deref()
                    .filter(|value| value.ends_with(".md"))
            })
            .map(|s| s.to_string());
        Ok(build_permissions_patch(&folder, root_index_path.as_deref()))
    })
}

fn ensure_index_file(
    state: &DailyState,
    rel_path: &str,
    title: &str,
    description: Option<&str>,
    part_of: Option<(&str, &str)>,
) -> Result<bool, String> {
    let fs_path = to_fs_path(rel_path, state.workspace_root.as_deref());
    let exists = host::fs::file_exists(&fs_path)?;

    let (mut fm, mut body) = if exists {
        let content = host::fs::read_file(&fs_path)?;
        parse_markdown(&content)?
    } else {
        (IndexMap::new(), String::new())
    };

    let mut changed = !exists;

    if fm.get("title").and_then(YamlValue::as_str) != Some(title) {
        fm.insert("title".to_string(), YamlValue::String(title.to_string()));
        changed = true;
    }

    if let Some(desc) = description
        && fm.get("description").and_then(YamlValue::as_str) != Some(desc)
    {
        fm.insert(
            "description".to_string(),
            YamlValue::String(desc.to_string()),
        );
        changed = true;
    }

    if let Some((parent_rel, parent_title)) = part_of {
        let parent_link = format_link_for(rel_path, parent_rel, parent_title, state.link_format);
        if fm.get("part_of").and_then(YamlValue::as_str) != Some(parent_link.as_str()) {
            fm.insert("part_of".to_string(), YamlValue::String(parent_link));
            changed = true;
        }
    }

    let contents = ensure_sequence(&mut fm, "contents");
    if fm.get("contents").is_none() || !matches!(fm.get("contents"), Some(YamlValue::Sequence(_))) {
        save_sequence(&mut fm, "contents", &contents);
        changed = true;
    }

    if body.trim().is_empty() {
        body = format!("\n# {title}\n");
        changed = true;
    }

    if changed {
        write_markdown(&fs_path, &fm, &body)?;
    }

    Ok(!exists)
}

fn add_to_contents(
    state: &DailyState,
    index_rel: &str,
    child_rel: &str,
    child_title: &str,
) -> Result<bool, String> {
    let fs_path = to_fs_path(index_rel, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path)?;
    let (mut fm, body) = parse_markdown(&content)?;

    let mut contents = ensure_sequence(&mut fm, "contents");
    let child_canonical = normalize_rel_path(child_rel);
    let new_entry = format_link_for(index_rel, child_rel, child_title, state.link_format);

    // Check for existing entries pointing to the same canonical path
    let mut found_exact = false;
    let mut stale_indices = Vec::new();

    for (i, entry) in contents.iter().enumerate() {
        let parsed = parse_link(entry);
        let canonical =
            to_canonical_with_link_format(&parsed, Path::new(index_rel), Some(state.link_format));
        if canonical == child_canonical {
            if *entry == new_entry {
                found_exact = true;
            } else {
                stale_indices.push(i);
            }
        }
    }

    if found_exact && stale_indices.is_empty() {
        return Ok(false);
    }

    // Remove stale entries (reverse order to preserve indices)
    for &i in stale_indices.iter().rev() {
        contents.remove(i);
    }

    if !found_exact {
        contents.push(new_entry);
    }

    save_sequence(&mut fm, "contents", &contents);
    write_markdown(&fs_path, &fm, &body)?;
    Ok(true)
}

fn set_part_of(
    state: &DailyState,
    child_rel: &str,
    parent_rel: &str,
    parent_title: &str,
) -> Result<(), String> {
    let fs_path = to_fs_path(child_rel, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path)?;
    let (mut fm, body) = parse_markdown(&content)?;
    let new_part_of = format_link_for(child_rel, parent_rel, parent_title, state.link_format);

    if fm.get("part_of").and_then(YamlValue::as_str) == Some(&new_part_of) {
        return Ok(());
    }

    fm.insert("part_of".to_string(), YamlValue::String(new_part_of));
    write_markdown(&fs_path, &fm, &body)
}

fn resolve_template_source(state: &DailyState) -> String {
    let Some(template) = state.config.entry_template.as_ref() else {
        return default_entry_template().to_string();
    };

    let trimmed = template.trim();
    if trimmed.contains('\n') || trimmed.contains("{{") {
        return trimmed.to_string();
    }

    let parsed = parse_link(trimmed);
    let path_candidate = if parsed.path.is_empty() {
        trimmed.to_string()
    } else {
        parsed.path
    };
    let fs_path = to_fs_path(&path_candidate, state.workspace_root.as_deref());
    match host::fs::read_file(&fs_path) {
        Ok(content) if !content.trim().is_empty() => content,
        _ => default_entry_template().to_string(),
    }
}

fn ensure_daily_entry_for_date(
    date: NaiveDate,
    state: &DailyState,
) -> Result<(String, bool), String> {
    let folder = state.config.effective_entry_folder();
    let paths = paths_for_date(&folder, date);
    let root_index_rel = find_existing_root_index_rel(state)?;

    let year_title = date.format("%Y").to_string();
    let month_title = date.format("%B %Y").to_string();
    let entry_title = date.format("%B %d, %Y").to_string();

    let root_part_of: Option<(String, String)> = root_index_rel.as_deref().map(|rel| {
        let title = read_title_from_file(state, rel).unwrap_or_else(|| "Index".to_string());
        (rel.to_string(), title)
    });

    ensure_index_file(
        state,
        &paths.daily_index,
        "Daily Index",
        Some("Date-based daily entry hierarchy"),
        root_part_of.as_ref().map(|(p, t)| (p.as_str(), t.as_str())),
    )?;
    ensure_index_file(
        state,
        &paths.year_index,
        &year_title,
        None,
        Some((&paths.daily_index, "Daily Index")),
    )?;
    ensure_index_file(
        state,
        &paths.month_index,
        &month_title,
        None,
        Some((&paths.year_index, &year_title)),
    )?;

    if let Some(root_rel) = root_index_rel.as_deref()
        && root_rel != paths.daily_index
    {
        add_to_contents(state, root_rel, &paths.daily_index, "Daily Index")?;
    }

    add_to_contents(state, &paths.daily_index, &paths.year_index, &year_title)?;
    add_to_contents(state, &paths.year_index, &paths.month_index, &month_title)?;

    let entry_fs_path = to_fs_path(&paths.entry, state.workspace_root.as_deref());
    let existed = host::fs::file_exists(&entry_fs_path)?;
    if !existed {
        let part_of = format_link_for(
            &paths.entry,
            &paths.month_index,
            &month_title,
            state.link_format,
        );
        let template = resolve_template_source(state);
        let now = current_local_datetime()?;
        let content = render_template(&template, &entry_title, date, &part_of, &now);
        host::fs::write_file(&entry_fs_path, &content)?;
    }

    set_part_of(state, &paths.entry, &paths.month_index, &month_title)?;
    add_to_contents(state, &paths.month_index, &paths.entry, &entry_title)?;

    Ok((paths.entry, !existed))
}

fn infer_entry_date(path_rel: &str, state: &DailyState) -> Result<NaiveDate, String> {
    let fs_path = to_fs_path(path_rel, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path)?;
    let (fm, _) = parse_markdown(&content)?;
    let now = current_local_datetime()?;

    for key in ["date", "created", "updated"] {
        if let Some(value) = fm.get(key)
            && let Some(raw) = value.as_str()
        {
            if key == "updated"
                && let Some(date) = parse_rfc3339_date_in_offset(raw, now.offset())
            {
                return Ok(date);
            }
            if let Ok(date) = parse_date_input(Some(raw), now.clone()) {
                return Ok(date);
            }
        }
    }

    parse_date_input(None, now).map_err(|e| e.to_string())
}

fn migrate_legacy_config(state_value: &mut DailyState) -> Result<(), String> {
    if state_value.config.migrated_legacy_config {
        return Ok(());
    }

    let mut migrated_any = false;

    for candidate in find_root_index_candidates(state_value.workspace_root.as_deref()) {
        if !host::fs::file_exists(&candidate)? {
            continue;
        }

        let content = host::fs::read_file(&candidate)?;
        let (mut fm, body) = parse_markdown(&content)?;
        let mut file_changed = false;

        if let Some(YamlValue::String(folder)) = fm.shift_remove("daily_entry_folder") {
            if state_value
                .config
                .entry_folder
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                state_value.config.entry_folder = Some(folder);
            }
            file_changed = true;
            migrated_any = true;
        }

        if let Some(YamlValue::String(template)) = fm.shift_remove("daily_template") {
            if state_value
                .config
                .entry_template
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                state_value.config.entry_template = Some(template);
            }
            file_changed = true;
            migrated_any = true;
        }

        if file_changed {
            write_markdown(&candidate, &fm, &body)?;
        }
    }

    if migrated_any {
        host::log::log(
            "info",
            "Migrated legacy daily workspace keys into plugin config",
        );
    }

    state_value.config.migrated_legacy_config = true;
    save_workspace_config(state_value)?;
    Ok(())
}

fn update_workspace_root(workspace_root: Option<String>) -> Result<(), String> {
    // Step 1: Always persist the workspace root and loaded config,
    // even if migration later fails.
    with_state_mut(|state| {
        state.workspace_root = workspace_root;
        state.config = load_workspace_config(state.workspace_root.as_deref());
        Ok(())
    })?;

    // Step 2: Read link_format from workspace root index.
    let link_format = {
        let state = current_state()?;
        read_link_format(&state)
    };
    with_state_mut(|state| {
        state.link_format = link_format;
        Ok(())
    })?;

    // Step 3: Attempt migration (reads/writes files). Non-fatal —
    // if this fails, the workspace root is still set from step 1.
    let migration_result = with_state_mut(|state| {
        migrate_legacy_config(state)?;
        save_workspace_config(state)?;
        Ok(())
    });
    if let Err(e) = migration_result {
        host::log::log("warn", &format!("Legacy config migration failed: {e}"));
    }
    Ok(())
}

fn get_component_html_by_id(component_id: &str) -> Option<&'static str> {
    match component_id {
        "daily.panel" => Some(include_str!("ui/panel.html")),
        _ => None,
    }
}

fn all_commands() -> Vec<String> {
    vec![
        "EnsureDailyEntry".to_string(),
        "GetAdjacentDailyEntry".to_string(),
        "GetEntryState".to_string(),
        "ImportEntriesToDaily".to_string(),
        "ListDailyEntryDates".to_string(),
        "OpenToday".to_string(),
        "OpenYesterday".to_string(),
        "UpdateConfig".to_string(),
        "CliDaily".to_string(),
        "get_component_html".to_string(),
    ]
}

/// Resolve a `contents` entry (markdown link or plain path) into a workspace-relative path.
///
/// `parent_rel` is the workspace-relative path of the file containing the `contents` array.
/// Workspace-root paths (starting with `/`) are resolved directly; relative paths are
/// resolved against the parent file's directory.
fn resolve_link_path(entry: &str, parent_rel: &str) -> String {
    let parsed = parse_link(entry);
    let raw = if parsed.path.is_empty() {
        entry.trim().to_string()
    } else {
        parsed.path
    };

    // Workspace-root paths (parse_link strips the leading `/`)
    // are already workspace-relative after normalize_rel_path.
    // Relative paths need to be joined with the parent's directory.
    if entry.contains("(/") || entry.contains("(</") || raw.starts_with('/') {
        // Workspace-root link — parse_link already stripped the `/`
        normalize_rel_path(&raw)
    } else {
        // Relative link — resolve against the parent file's directory
        let parent_dir = Path::new(parent_rel).parent().unwrap_or(Path::new(""));
        let joined = parent_dir.join(&raw);
        normalize_rel_path(&joined.to_string_lossy())
    }
}

/// Walk the contents tree from `daily_index.md` → year index → month index
/// to find the month index path for a given year/month.
///
/// Returns `Ok(Some(path))` if found, `Ok(None)` if the tree doesn't contain
/// a matching year or month, `Err` on read failures.
fn find_month_index_via_tree(
    state: &DailyState,
    year: i32,
    month: u32,
) -> Result<Option<String>, String> {
    let folder = state.config.effective_entry_folder();
    let daily_index_rel = daily_logic::scoped_path(&folder, "daily_index.md");
    let daily_index_fs = to_fs_path(&daily_index_rel, state.workspace_root.as_deref());

    // Read daily_index.md
    let content = match host::fs::read_file(&daily_index_fs) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let (fm, _) = parse_markdown(&content)?;
    let daily_contents = ensure_sequence(&mut fm.clone(), "contents");

    // Find matching year index
    let year_str = format!("{year}");
    let year_segment = format!("/{year}/");
    let mut year_index_rel = None;
    for entry in &daily_contents {
        let path = resolve_link_path(entry, &daily_index_rel);
        if path.contains(&year_segment) || path.contains(&format!("/{year_str}_")) {
            year_index_rel = Some(path);
            break;
        }
    }

    let year_index_rel = match year_index_rel {
        Some(p) => p,
        None => return Ok(None),
    };

    // Read year index
    let year_index_fs = to_fs_path(&year_index_rel, state.workspace_root.as_deref());
    let content = match host::fs::read_file(&year_index_fs) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let (fm, _) = parse_markdown(&content)?;
    let year_contents = ensure_sequence(&mut fm.clone(), "contents");

    // Find matching month index
    let month_segment = format!("/{month:02}/");
    for entry in &year_contents {
        let path = resolve_link_path(entry, &year_index_rel);
        if path.contains(&month_segment) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Read a month index file and extract day numbers from its `contents` entries
/// that match the given year/month.
fn dates_from_month_index(
    state: &DailyState,
    month_index_rel: &str,
    year: i32,
    month: u32,
) -> Result<Vec<u32>, String> {
    let fs_path = to_fs_path(month_index_rel, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path)?;
    let (fm, _) = parse_markdown(&content)?;
    let contents = ensure_sequence(&mut fm.clone(), "contents");

    let mut days = Vec::new();
    for entry in &contents {
        let path = resolve_link_path(entry, month_index_rel);

        // Try strict path_to_date first, then fall back to filename parsing
        let date = path_to_date(&path).or_else(|_| date_from_filename(&path));

        if let Ok(date) = date {
            if date.year() == year && date.month() == month {
                days.push(date.day());
            }
        }
    }

    Ok(days)
}

fn dispatch_command(command: &str, params: JsonValue) -> Result<JsonValue, String> {
    let state = current_state()?;

    match command {
        "EnsureDailyEntry" => {
            let now = current_local_datetime()?;
            let date = parse_date_input(params.get("date").and_then(|v| v.as_str()), now)
                .map_err(|e| e.to_string())?;
            let (path, created) = ensure_daily_entry_for_date(date, &state)?;
            Ok(serde_json::json!({
                "path": path,
                "created": created,
                "date": date.format("%Y-%m-%d").to_string(),
            }))
        }
        "GetAdjacentDailyEntry" => {
            let input_path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("GetAdjacentDailyEntry requires `path`")?;

            let direction = match params
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("next")
            {
                "prev" | "previous" => DailyDirection::Prev,
                _ => DailyDirection::Next,
            };

            let ensure = params
                .get("ensure")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let rel_input = to_workspace_rel(input_path, state.workspace_root.as_deref());
            let adjacent_rel =
                adjacent_daily_entry_path(&rel_input, direction).map_err(|e| e.to_string())?;

            if ensure {
                let date = path_to_date(&adjacent_rel).map_err(|e| e.to_string())?;
                let (path, created) = ensure_daily_entry_for_date(date, &state)?;
                Ok(serde_json::json!({
                    "path": path,
                    "created": created,
                    "date": date.format("%Y-%m-%d").to_string(),
                }))
            } else {
                Ok(serde_json::json!({
                    "path": adjacent_rel,
                }))
            }
        }
        "GetEntryState" => {
            let input_path = params
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("GetEntryState requires `path`")?;
            let rel_path = to_workspace_rel(input_path, state.workspace_root.as_deref());
            if let Ok(date) = path_to_date(&rel_path) {
                let today = current_local_date()?;
                Ok(serde_json::json!({
                    "is_daily": true,
                    "is_today": date == today,
                    "date": date.format("%Y-%m-%d").to_string(),
                }))
            } else {
                Ok(serde_json::json!({
                    "is_daily": false,
                    "is_today": false,
                    "date": JsonValue::Null,
                }))
            }
        }
        "ImportEntriesToDaily" => {
            let dry_run = params
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let entries = params
                .get("entries")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut imported = 0usize;
            let mut errors = Vec::new();

            for entry in entries {
                let (path_raw, explicit_date) = if let Some(path) = entry.as_str() {
                    (path.to_string(), None)
                } else {
                    let path = entry
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let date = entry
                        .get("date")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    (path, date)
                };

                if path_raw.trim().is_empty() {
                    continue;
                }

                let rel_path = to_workspace_rel(&path_raw, state.workspace_root.as_deref());
                let now = current_local_datetime()?;
                let date = match explicit_date {
                    Some(date) => parse_date_input(Some(&date), now).map_err(|e| e.to_string()),
                    None => infer_entry_date(&rel_path, &state),
                };

                let date = match date {
                    Ok(date) => date,
                    Err(e) => {
                        errors.push(format!("{rel_path}: {e}"));
                        continue;
                    }
                };

                let (daily_path, _) = match ensure_daily_entry_for_date(date, &state) {
                    Ok(value) => value,
                    Err(e) => {
                        errors.push(format!("{rel_path}: {e}"));
                        continue;
                    }
                };

                if !dry_run {
                    let parent_title = date.format("%B %d, %Y").to_string();
                    let entry_title =
                        read_title_from_file(&state, &rel_path).unwrap_or_else(|| {
                            Path::new(&rel_path)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| rel_path.clone())
                        });
                    if let Err(e) = set_part_of(&state, &rel_path, &daily_path, &parent_title) {
                        errors.push(format!("{rel_path}: {e}"));
                        continue;
                    }
                    if let Err(e) = add_to_contents(&state, &daily_path, &rel_path, &entry_title) {
                        errors.push(format!("{rel_path}: {e}"));
                        continue;
                    }
                }

                imported += 1;
            }

            Ok(serde_json::json!({
                "imported": imported,
                "errors": errors,
                "dry_run": dry_run,
            }))
        }
        "OpenToday" => {
            let (path, created) = ensure_daily_entry_for_date(current_local_date()?, &state)?;
            Ok(serde_json::json!({
                "__diaryx_cli_action": "open_entry",
                "path": path,
                "created": created,
            }))
        }
        "OpenYesterday" => {
            let date = current_local_date()? - Duration::days(1);
            let (path, created) = ensure_daily_entry_for_date(date, &state)?;
            Ok(serde_json::json!({
                "__diaryx_cli_action": "open_entry",
                "path": path,
                "created": created,
            }))
        }
        "UpdateConfig" => handle_update_config(params),
        "CliDaily" => {
            let now = current_local_datetime()?;
            let date = parse_date_input(params.get("date").and_then(|v| v.as_str()), now)
                .map_err(|e| e.to_string())?;
            let print = params
                .get("print")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let (path, created) = ensure_daily_entry_for_date(date, &state)?;
            if print {
                let content =
                    host::fs::read_file(&to_fs_path(&path, state.workspace_root.as_deref()))?;
                Ok(serde_json::json!({
                    "__diaryx_cli_action": "print",
                    "text": content,
                    "path": path,
                    "created": created,
                }))
            } else {
                Ok(serde_json::json!({
                    "__diaryx_cli_action": "open_entry",
                    "path": path,
                    "created": created,
                }))
            }
        }
        "ListDailyEntryDates" => {
            let year = params
                .get("year")
                .and_then(|v| v.as_i64())
                .ok_or("ListDailyEntryDates requires `year`")? as i32;
            let month = params
                .get("month")
                .and_then(|v| v.as_i64())
                .ok_or("ListDailyEntryDates requires `month`")? as u32;
            if !(1..=12).contains(&month) {
                return Err("month must be 1-12".to_string());
            }
            let folder = state.config.effective_entry_folder();

            let mut dates: Vec<u32> = Vec::new();

            // Phase 1: Tree walk (primary) — reads daily_index → year index → month index
            if let Ok(Some(month_index_rel)) = find_month_index_via_tree(&state, year, month) {
                if let Ok(tree_dates) =
                    dates_from_month_index(&state, &month_index_rel, year, month)
                {
                    dates.extend(tree_dates);
                }
            }

            // Phase 2: Filesystem scan (supplement) — catches unlisted entries
            let prefix = to_fs_path(
                &format!("{folder}/{year}/{month:02}/"),
                state.workspace_root.as_deref(),
            );
            let files = host::fs::list_files(&prefix).unwrap_or_default();
            for file in &files {
                if let Ok(date) = path_to_date(file) {
                    if date.year() == year && date.month() == month {
                        dates.push(date.day());
                    }
                }
            }

            dates.sort();
            dates.dedup();
            Ok(serde_json::json!({
                "year": year,
                "month": month,
                "dates": dates,
                "folder": folder,
            }))
        }
        "get_component_html" => {
            let component_id = params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or("daily.panel");
            let html = get_component_html_by_id(component_id)
                .ok_or_else(|| format!("Unknown component id: {component_id}"))?;
            Ok(JsonValue::String(html.to_string()))
        }
        _ => Err(format!("Unknown command: {command}")),
    }
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.daily",
        "Daily",
        env!("CARGO_PKG_VERSION"),
        "Daily entry plugin with date hierarchy, navigation, and CLI surface",
        vec!["workspace_events".into(), "custom_commands".into()],
    )
    .ui(vec![
        serde_json::json!({
            "slot": "SidebarTab",
            "id": "daily-panel",
            "label": "Daily",
            "icon": "calendar-days",
            "side": "Left",
            "component": {
                "type": "Iframe",
                "component_id": "daily.panel",
            },
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "daily-open-today",
            "label": "Open Today's Entry",
            "group": "Daily",
            "plugin_command": "OpenToday",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "daily-open-yesterday",
            "label": "Open Yesterday's Entry",
            "group": "Daily",
            "plugin_command": "OpenYesterday",
        }),
    ])
    .commands(all_commands())
    .cli(vec![serde_json::json!({
        "name": "daily",
        "about": "Open or print a daily entry",
        "aliases": ["d"],
        "command_name": "CliDaily",
        "requires_workspace": true,
        "args": [
            {
                "name": "date",
                "help": "Date expression (today, yesterday, YYYY-MM-DD)",
                "required": false,
                "value_type": "String"
            },
            {
                "name": "print",
                "help": "Print entry content instead of launching editor",
                "short": "p",
                "long": "print",
                "is_flag": true
            }
        ]
    })])
    .requested_permissions(build_requested_permissions(
        &DailyPluginConfig::default().effective_entry_folder(),
        Some("README.md"),
    ));

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    update_workspace_root(params.workspace_root).map_err(extism_pdk::Error::msg)?;
    host::log::log("info", "Daily plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host::log::log("info", "Daily plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = match dispatch_command(&req.command, req.params) {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
    };

    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    let parsed: JsonValue = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid JSON: {e}")))?;

    let cmd_type = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| extism_pdk::Error::msg("Missing `type` in typed command"))?;

    let params = parsed.get("params").cloned().unwrap_or(JsonValue::Null);
    match dispatch_command(cmd_type, params) {
        Ok(data) => {
            let response = serde_json::json!({
                "type": "PluginResult",
                "data": data
            });
            Ok(serde_json::to_string(&response)?)
        }
        Err(_) => Ok(String::new()),
    }
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let state = current_state().map_err(extism_pdk::Error::msg)?;
    Ok(serde_json::to_string(&state.config)?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    with_state_mut(|state| {
        let mut config: DailyPluginConfig = serde_json::from_str(&input).unwrap_or_default();

        if let Some(folder) = config.entry_folder.as_deref() {
            config.entry_folder = Some(folder.trim_matches('/').to_string());
        }

        state.config.entry_folder = config.entry_folder;
        state.config.entry_template = config.entry_template;
        if config.migrated_legacy_config {
            state.config.migrated_legacy_config = true;
        }

        save_workspace_config(state)?;
        Ok(())
    })
    .map_err(extism_pdk::Error::msg)?;
    Ok(String::new())
}

#[plugin_fn]
pub fn get_component_html(input: String) -> FnResult<String> {
    if input.trim().is_empty() {
        return Ok(include_str!("ui/panel.html").to_string());
    }

    if input.trim_start().starts_with('{') {
        let parsed: JsonValue = serde_json::from_str(&input)?;
        let component_id = parsed
            .get("component_id")
            .and_then(|v| v.as_str())
            .unwrap_or("daily.panel");
        if let Some(html) = get_component_html_by_id(component_id) {
            return Ok(html.to_string());
        }
        return Err(extism_pdk::Error::msg(format!("Unknown component id: {component_id}")).into());
    }

    if let Some(html) = get_component_html_by_id(input.trim()) {
        return Ok(html.to_string());
    }

    Err(extism_pdk::Error::msg(format!("Unknown component id: {}", input.trim())).into())
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: JsonValue = serde_json::from_str(&input).unwrap_or(JsonValue::Null);
    let event_type = event
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if matches!(event_type, "workspace_opened" | "workspace_changed") {
        let workspace_root = event
            .get("payload")
            .and_then(|v| v.get("workspace_root"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let _ = update_workspace_root(workspace_root);
    }

    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_requested_permissions_scope_daily_folder_and_root_index() {
        let permissions = requested_permissions_for("Daily", Some("README.md"));
        let read_include = permissions["defaults"]["read_files"]["include"]
            .as_array()
            .expect("read include array");
        let create_include = permissions["defaults"]["create_files"]["include"]
            .as_array()
            .expect("create include array");

        assert_eq!(read_include.len(), 2);
        assert_eq!(read_include[0].as_str(), Some("Daily"));
        assert_eq!(read_include[1].as_str(), Some("README.md"));
        assert_eq!(create_include[0].as_str(), Some("Daily"));
    }

    #[test]
    fn resolve_link_path_workspace_root_link() {
        // Workspace-root markdown link — path starts with /
        let resolved = resolve_link_path(
            "[2026 Index](/Daily/2026/2026_index.md)",
            "Daily/daily_index.md",
        );
        assert_eq!(resolved, "Daily/2026/2026_index.md");
    }

    #[test]
    fn resolve_link_path_relative_link() {
        // Relative link stored by add_to_contents — no leading /
        let resolved = resolve_link_path("2026/2026_index.md", "Daily/daily_index.md");
        assert_eq!(resolved, "Daily/2026/2026_index.md");
    }

    #[test]
    fn resolve_link_path_angle_bracket_root_link() {
        // Angle-bracket workspace-root link (spaces in filename)
        let resolved = resolve_link_path(
            "[2025 03 Entries](</Daily/2025/03/2025-03 entries.md>)",
            "Daily/2025/2025_index.md",
        );
        assert_eq!(resolved, "Daily/2025/03/2025-03 entries.md");
    }

    #[test]
    fn resolve_link_path_relative_entry_from_month_index() {
        // Relative entry link from a month index
        let resolved = resolve_link_path("2025-09-19.md", "Daily/2025/09/09.md");
        assert_eq!(resolved, "Daily/2025/09/2025-09-19.md");
    }

    #[test]
    fn permissions_patch_replaces_daily_file_rules() {
        let patch = build_permissions_patch("Journal/Daily", Some("/README.md"));

        assert_eq!(
            patch["plugin_permissions_patch"]["mode"].as_str(),
            Some("replace")
        );
        assert_eq!(
            patch["plugin_permissions_patch"]["permissions"]["edit_files"]["include"][0].as_str(),
            Some("Journal/Daily")
        );
        assert_eq!(
            patch["plugin_permissions_patch"]["permissions"]["edit_files"]["include"][1].as_str(),
            Some("README.md")
        );
    }
}
