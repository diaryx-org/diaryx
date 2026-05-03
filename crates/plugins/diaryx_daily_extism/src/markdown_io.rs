//! Markdown/frontmatter I/O helpers.

use diaryx_core::frontmatter;
use diaryx_core::yaml;
use diaryx_plugin_sdk::prelude::*;
use indexmap::IndexMap;

use crate::paths::to_fs_path;
use crate::state::DailyState;

pub fn parse_markdown(content: &str) -> Result<(IndexMap<String, yaml::Value>, String), String> {
    let parsed = frontmatter::parse_or_empty(content).map_err(|e| e.to_string())?;
    Ok((parsed.frontmatter, parsed.body))
}

pub fn write_markdown(
    fs_path: &str,
    frontmatter_map: &IndexMap<String, yaml::Value>,
    body: &str,
) -> Result<(), String> {
    let serialized = frontmatter::serialize(frontmatter_map, body).map_err(|e| e.to_string())?;
    host::fs::write_file(fs_path, &serialized)
}

pub fn ensure_sequence(
    frontmatter_map: &mut IndexMap<String, yaml::Value>,
    key: &str,
) -> Vec<String> {
    match frontmatter_map.get(key) {
        Some(yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn save_sequence(
    frontmatter_map: &mut IndexMap<String, yaml::Value>,
    key: &str,
    values: &[String],
) {
    let seq = values
        .iter()
        .map(|v| yaml::Value::String(v.clone()))
        .collect::<Vec<_>>();
    frontmatter_map.insert(key.to_string(), yaml::Value::Sequence(seq));
}

pub fn read_title_from_file(state: &DailyState, rel_path: &str) -> Option<String> {
    let fs_path = to_fs_path(rel_path, state.workspace_root.as_deref());
    let content = host::fs::read_file(&fs_path).ok()?;
    let (fm, _) = parse_markdown(&content).ok()?;
    fm.get("title")
        .and_then(yaml::Value::as_str)
        .map(|s| s.to_string())
}
