//! Extism guest plugin for Diaryx daily entry functionality.

pub mod host_bridge;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use chrono::{Datelike, Local, NaiveDate};
use diaryx_core::frontmatter;
use diaryx_core::link_parser::parse_link;
use diaryx_daily::{
    DailyDirection, DailyPluginConfig, adjacent_daily_entry_path, default_entry_template,
    parse_date_input, path_to_date, paths_for_date, render_template,
};
use extism_pdk::*;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestManifest {
    id: String,
    name: String,
    version: String,
    description: String,
    capabilities: Vec<String>,
    #[serde(default)]
    ui: Vec<JsonValue>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    cli: Vec<JsonValue>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandRequest {
    command: String,
    params: JsonValue,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandResponse {
    success: bool,
    #[serde(default)]
    data: Option<JsonValue>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct DailyState {
    workspace_root: Option<String>,
    config: DailyPluginConfig,
}

static STATE: OnceLock<Mutex<DailyState>> = OnceLock::new();

fn state() -> &'static Mutex<DailyState> {
    STATE.get_or_init(|| Mutex::new(DailyState::default()))
}

fn current_state() -> Result<DailyState, String> {
    let guard = state()
        .lock()
        .map_err(|_| "daily plugin state lock poisoned".to_string())?;
    Ok(guard.clone())
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
    match host_bridge::storage_get(&key) {
        Ok(Some(bytes)) => serde_json::from_slice::<DailyPluginConfig>(&bytes).unwrap_or_default(),
        _ => DailyPluginConfig::default(),
    }
}

fn save_workspace_config(state: &DailyState) -> Result<(), String> {
    let key = storage_key_for_workspace(state.workspace_root.as_deref());
    let bytes = serde_json::to_vec(&state.config).map_err(|e| format!("serialize config: {e}"))?;
    host_bridge::storage_set(&key, &bytes)?;
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
    host_bridge::write_file(fs_path, &serialized)
}

fn relative_ref(from_file_rel: &str, to_file_rel: &str) -> String {
    let from_dir = Path::new(from_file_rel).parent().unwrap_or(Path::new(""));
    let to_path = Path::new(to_file_rel);
    let rel = pathdiff::diff_paths(to_path, from_dir).unwrap_or_else(|| to_path.to_path_buf());
    rel.to_string_lossy().replace('\\', "/")
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

fn ensure_index_file(
    state: &DailyState,
    rel_path: &str,
    title: &str,
    description: Option<&str>,
    part_of: Option<&str>,
) -> Result<bool, String> {
    let fs_path = to_fs_path(rel_path, state.workspace_root.as_deref());
    let exists = host_bridge::file_exists(&fs_path)?;

    let (mut fm, mut body) = if exists {
        let content = host_bridge::read_file(&fs_path)?;
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

    if let Some(parent_rel) = part_of {
        let parent_ref = relative_ref(rel_path, parent_rel);
        if fm.get("part_of").and_then(YamlValue::as_str) != Some(parent_ref.as_str()) {
            fm.insert("part_of".to_string(), YamlValue::String(parent_ref));
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

fn add_to_contents(state: &DailyState, index_rel: &str, child_rel: &str) -> Result<bool, String> {
    let fs_path = to_fs_path(index_rel, state.workspace_root.as_deref());
    let content = host_bridge::read_file(&fs_path)?;
    let (mut fm, body) = parse_markdown(&content)?;

    let mut contents = ensure_sequence(&mut fm, "contents");
    let target = relative_ref(index_rel, child_rel);
    if contents.iter().any(|c| c == &target) {
        return Ok(false);
    }

    contents.push(target);
    save_sequence(&mut fm, "contents", &contents);
    write_markdown(&fs_path, &fm, &body)?;
    Ok(true)
}

fn set_part_of(state: &DailyState, child_rel: &str, parent_rel: &str) -> Result<(), String> {
    let fs_path = to_fs_path(child_rel, state.workspace_root.as_deref());
    let content = host_bridge::read_file(&fs_path)?;
    let (mut fm, body) = parse_markdown(&content)?;
    let part_of = relative_ref(child_rel, parent_rel);
    fm.insert("part_of".to_string(), YamlValue::String(part_of));
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
    match host_bridge::read_file(&fs_path) {
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

    let year_title = date.format("%Y").to_string();
    let month_title = date.format("%B %Y").to_string();

    ensure_index_file(
        state,
        &paths.daily_index,
        "Daily Index",
        Some("Date-based daily entry hierarchy"),
        None,
    )?;
    ensure_index_file(
        state,
        &paths.year_index,
        &year_title,
        None,
        Some(&paths.daily_index),
    )?;
    ensure_index_file(
        state,
        &paths.month_index,
        &month_title,
        None,
        Some(&paths.year_index),
    )?;

    add_to_contents(state, &paths.daily_index, &paths.year_index)?;
    add_to_contents(state, &paths.year_index, &paths.month_index)?;

    let entry_fs_path = to_fs_path(&paths.entry, state.workspace_root.as_deref());
    let existed = host_bridge::file_exists(&entry_fs_path)?;
    if !existed {
        let part_of = relative_ref(&paths.entry, &paths.month_index);
        let title = date.format("%B %d, %Y").to_string();
        let template = resolve_template_source(state);
        let content = render_template(&template, &title, date, &part_of);
        host_bridge::write_file(&entry_fs_path, &content)?;
    }

    set_part_of(state, &paths.entry, &paths.month_index)?;
    add_to_contents(state, &paths.month_index, &paths.entry)?;

    Ok((paths.entry, !existed))
}

fn infer_entry_date(path_rel: &str, state: &DailyState) -> Result<NaiveDate, String> {
    let fs_path = to_fs_path(path_rel, state.workspace_root.as_deref());
    let content = host_bridge::read_file(&fs_path)?;
    let (fm, _) = parse_markdown(&content)?;

    for key in ["date", "created", "updated"] {
        if let Some(value) = fm.get(key)
            && let Some(raw) = value.as_str()
            && let Ok(date) = parse_date_input(Some(raw))
        {
            return Ok(date);
        }
    }

    parse_date_input(None).map_err(|e| e.to_string())
}

fn migrate_legacy_config(state_value: &mut DailyState) -> Result<(), String> {
    if state_value.config.migrated_legacy_config {
        return Ok(());
    }

    let mut migrated_any = false;

    for candidate in find_root_index_candidates(state_value.workspace_root.as_deref()) {
        if !host_bridge::file_exists(&candidate)? {
            continue;
        }

        let content = host_bridge::read_file(&candidate)?;
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
        host_bridge::log_message(
            "info",
            "Migrated legacy daily workspace keys into plugin config",
        );
    }

    state_value.config.migrated_legacy_config = true;
    save_workspace_config(state_value)?;
    Ok(())
}

fn update_workspace_root(workspace_root: Option<String>) -> Result<(), String> {
    let mut guard = state()
        .lock()
        .map_err(|_| "daily plugin state lock poisoned".to_string())?;

    guard.workspace_root = workspace_root;
    guard.config = load_workspace_config(guard.workspace_root.as_deref());
    migrate_legacy_config(&mut guard)?;
    save_workspace_config(&guard)?;
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
        "CliDaily".to_string(),
        "get_component_html".to_string(),
    ]
}

fn dispatch_command(command: &str, params: JsonValue) -> Result<JsonValue, String> {
    let state = current_state()?;

    match command {
        "EnsureDailyEntry" => {
            let date = parse_date_input(params.get("date").and_then(|v| v.as_str()))
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
                let today = Local::now().date_naive();
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
                let date = match explicit_date {
                    Some(date) => parse_date_input(Some(&date)).map_err(|e| e.to_string()),
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
                    if let Err(e) = set_part_of(&state, &rel_path, &daily_path) {
                        errors.push(format!("{rel_path}: {e}"));
                        continue;
                    }
                    if let Err(e) = add_to_contents(&state, &daily_path, &rel_path) {
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
            let (path, created) = ensure_daily_entry_for_date(Local::now().date_naive(), &state)?;
            Ok(serde_json::json!({
                "__diaryx_cli_action": "open_entry",
                "path": path,
                "created": created,
            }))
        }
        "OpenYesterday" => {
            let date = Local::now().date_naive() - chrono::Duration::days(1);
            let (path, created) = ensure_daily_entry_for_date(date, &state)?;
            Ok(serde_json::json!({
                "__diaryx_cli_action": "open_entry",
                "path": path,
                "created": created,
            }))
        }
        "CliDaily" => {
            let date = parse_date_input(params.get("date").and_then(|v| v.as_str()))
                .map_err(|e| e.to_string())?;
            let print = params
                .get("print")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let (path, created) = ensure_daily_entry_for_date(date, &state)?;
            if print {
                let content =
                    host_bridge::read_file(&to_fs_path(&path, state.workspace_root.as_deref()))?;
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
            let prefix = to_fs_path(
                &format!("{folder}/{year}/{month:02}/"),
                state.workspace_root.as_deref(),
            );
            let files = host_bridge::list_files(&prefix).unwrap_or_default();
            let mut dates: Vec<u32> = Vec::new();
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
    let manifest = GuestManifest {
        id: "diaryx.daily".into(),
        name: "Daily".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Daily entry plugin with date hierarchy, navigation, and CLI surface".into(),
        capabilities: vec!["workspace_events".into(), "custom_commands".into()],
        ui: vec![
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
        ],
        commands: all_commands(),
        cli: vec![serde_json::json!({
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
        })],
    };

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    update_workspace_root(params.workspace_root).map_err(extism_pdk::Error::msg)?;
    host_bridge::log_message("info", "Daily plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host_bridge::log_message("info", "Daily plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = match dispatch_command(&req.command, req.params) {
        Ok(data) => CommandResponse {
            success: true,
            data: Some(data),
            error: None,
        },
        Err(error) => CommandResponse {
            success: false,
            data: None,
            error: Some(error),
        },
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
    let mut guard = state()
        .lock()
        .map_err(|_| extism_pdk::Error::msg("daily plugin state lock poisoned"))?;
    let mut config: DailyPluginConfig = serde_json::from_str(&input).unwrap_or_default();

    if let Some(folder) = config.entry_folder.as_deref() {
        config.entry_folder = Some(folder.trim_matches('/').to_string());
    }

    guard.config.entry_folder = config.entry_folder;
    guard.config.entry_template = config.entry_template;
    if config.migrated_legacy_config {
        guard.config.migrated_legacy_config = true;
    }

    save_workspace_config(&guard).map_err(extism_pdk::Error::msg)?;
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
