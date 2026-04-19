//! Plugin command dispatch: maps JSON command names to typed handlers.

use std::path::Path;

use chrono::{Datelike, Duration};
use diaryx_plugin_sdk::prelude::*;
use serde_json::Value as JsonValue;

use crate::daily_logic::{DailyDirection, adjacent_daily_entry_path, parse_date_input, path_to_date};
use crate::indices::{
    add_to_contents, dates_from_month_index, ensure_daily_entry_for_date,
    find_month_index_via_tree, infer_entry_date, set_part_of,
};
use crate::markdown_io::read_title_from_file;
use crate::paths::{to_fs_path, to_workspace_rel};
use crate::permissions::handle_update_config;
use crate::state::{current_local_date, current_local_datetime, current_state};

pub fn all_commands() -> Vec<String> {
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

pub fn get_component_html_by_id(component_id: &str) -> Option<&'static str> {
    match component_id {
        "daily.panel" => Some(include_str!("ui/panel.html")),
        _ => None,
    }
}

pub fn dispatch_command(command: &str, params: JsonValue) -> Result<JsonValue, String> {
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
