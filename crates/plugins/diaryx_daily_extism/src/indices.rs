//! Daily-hierarchy index files, `contents` maintenance, and tree walks.

use std::path::Path;

use chrono::{Datelike, NaiveDate};
use diaryx_core::link_parser::{parse_link, to_canonical_with_link_format};
use diaryx_core::yaml;
use diaryx_plugin_sdk::prelude::*;
use indexmap::IndexMap;

use crate::daily_logic::{
    date_from_filename, default_entry_template, parse_date_input, parse_rfc3339_date_in_offset,
    path_to_date, paths_for_date, render_template, scoped_path,
};
use crate::links::{format_link_for, resolve_link_path};
use crate::markdown_io::{
    ensure_sequence, parse_markdown, read_title_from_file, save_sequence, write_markdown,
};
use crate::paths::{find_existing_root_index_rel, normalize_rel_path, to_fs_path};
use crate::state::{DailyState, current_local_datetime};

pub fn ensure_index_file(
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

    if fm.get("title").and_then(yaml::Value::as_str) != Some(title) {
        fm.insert("title".to_string(), yaml::Value::String(title.to_string()));
        changed = true;
    }

    if let Some(desc) = description
        && fm.get("description").and_then(yaml::Value::as_str) != Some(desc)
    {
        fm.insert(
            "description".to_string(),
            yaml::Value::String(desc.to_string()),
        );
        changed = true;
    }

    if let Some((parent_rel, parent_title)) = part_of {
        let parent_link = format_link_for(rel_path, parent_rel, parent_title, state.link_format);
        if fm.get("part_of").and_then(yaml::Value::as_str) != Some(parent_link.as_str()) {
            fm.insert("part_of".to_string(), yaml::Value::String(parent_link));
            changed = true;
        }
    }

    let contents = ensure_sequence(&mut fm, "contents");
    if fm.get("contents").is_none() || !matches!(fm.get("contents"), Some(yaml::Value::Sequence(_)))
    {
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

pub fn add_to_contents(
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

pub fn set_part_of(
    state: &DailyState,
    child_rel: &str,
    parent_rel: &str,
    parent_title: &str,
) -> Result<(), String> {
    let fs_path = to_fs_path(child_rel, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path)?;
    let (mut fm, body) = parse_markdown(&content)?;
    let new_part_of = format_link_for(child_rel, parent_rel, parent_title, state.link_format);

    if fm.get("part_of").and_then(yaml::Value::as_str) == Some(&new_part_of) {
        return Ok(());
    }

    fm.insert("part_of".to_string(), yaml::Value::String(new_part_of));
    write_markdown(&fs_path, &fm, &body)
}

pub fn resolve_template_source(state: &DailyState) -> String {
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

pub fn ensure_daily_entry_for_date(
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

pub fn infer_entry_date(path_rel: &str, state: &DailyState) -> Result<NaiveDate, String> {
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

/// Walk the contents tree from `daily_index.md` → year index → month index
/// to find the month index path for a given year/month.
///
/// Returns `Ok(Some(path))` if found, `Ok(None)` if the tree doesn't contain
/// a matching year or month, `Err` on read failures.
pub fn find_month_index_via_tree(
    state: &DailyState,
    year: i32,
    month: u32,
) -> Result<Option<String>, String> {
    let folder = state.config.effective_entry_folder();
    let daily_index_rel = scoped_path(&folder, "daily_index.md");
    let daily_index_fs = to_fs_path(&daily_index_rel, state.workspace_root.as_deref());

    let content = match host::fs::read_file(&daily_index_fs) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let (fm, _) = parse_markdown(&content)?;
    let daily_contents = ensure_sequence(&mut fm.clone(), "contents");

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

    let year_index_fs = to_fs_path(&year_index_rel, state.workspace_root.as_deref());
    let content = match host::fs::read_file(&year_index_fs) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let (fm, _) = parse_markdown(&content)?;
    let year_contents = ensure_sequence(&mut fm.clone(), "contents");

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
pub fn dates_from_month_index(
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

        let date = path_to_date(&path).or_else(|_| date_from_filename(&path));

        if let Ok(date) = date {
            if date.year() == year && date.month() == month {
                days.push(date.day());
            }
        }
    }

    Ok(days)
}
