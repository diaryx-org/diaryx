//! Workspace/filesystem path conversion helpers.

use std::path::Path;

use diaryx_plugin_sdk::prelude::*;

use crate::state::DailyState;

pub fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

pub fn is_absolute_path(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    path.len() > 1 && path.as_bytes()[1] == b':'
}

pub fn to_fs_path(rel_path: &str, workspace_root: Option<&str>) -> String {
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

pub fn to_workspace_rel(path: &str, workspace_root: Option<&str>) -> String {
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

pub fn root_index_scope(path: Option<&str>) -> String {
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

pub fn find_root_index_candidates(workspace_root: Option<&str>) -> Vec<String> {
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

pub fn root_index_rel_candidates(state: &DailyState) -> Vec<String> {
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

pub fn find_existing_root_index_rel(state: &DailyState) -> Result<Option<String>, String> {
    for candidate in root_index_rel_candidates(state) {
        let fs_path = to_fs_path(&candidate, state.workspace_root.as_deref());
        if host::fs::file_exists(&fs_path)? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}
