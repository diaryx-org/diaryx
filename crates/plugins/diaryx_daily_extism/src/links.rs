//! Link parsing, formatting, and workspace link-format discovery.

use std::path::Path;

use diaryx_core::link_parser::{LinkFormat, format_link_with_format, parse_link};
use diaryx_core::yaml;
use diaryx_plugin_sdk::prelude::*;

use crate::markdown_io::parse_markdown;
use crate::paths::{find_existing_root_index_rel, normalize_rel_path, to_fs_path};
use crate::state::DailyState;

pub fn parse_link_format_str(s: &str) -> Option<LinkFormat> {
    match s {
        "markdown_root" => Some(LinkFormat::MarkdownRoot),
        "markdown_relative" => Some(LinkFormat::MarkdownRelative),
        "plain_relative" => Some(LinkFormat::PlainRelative),
        "plain_canonical" => Some(LinkFormat::PlainCanonical),
        _ => None,
    }
}

pub fn read_link_format(state: &DailyState) -> LinkFormat {
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
        Some(yaml::Value::Mapping(config_map)) => {
            if let Some(yaml::Value::String(s)) = config_map.get("link_format") {
                if let Some(fmt) = parse_link_format_str(s) {
                    return fmt;
                }
            }
        }
        Some(yaml::Value::String(link_str)) => {
            // File link to workspace config (e.g., "[Config](/Meta/Config.md)")
            let config_rel = resolve_link_path(link_str, &root_rel);
            let config_fs = to_fs_path(&config_rel, state.workspace_root.as_deref());
            if let Ok(config_content) = host::fs::read_file(&config_fs) {
                if let Ok((config_fm, _)) = parse_markdown(&config_content) {
                    if let Some(yaml::Value::String(s)) = config_fm.get("link_format") {
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
    if let Some(yaml::Value::String(s)) = fm.get("link_format") {
        if let Some(fmt) = parse_link_format_str(s) {
            return fmt;
        }
    }

    LinkFormat::default()
}

pub fn format_link_for(from_rel: &str, to_rel: &str, title: &str, format: LinkFormat) -> String {
    format_link_with_format(to_rel, title, format, from_rel)
}

/// Resolve a `contents` entry (markdown link or plain path) into a workspace-relative path.
///
/// `parent_rel` is the workspace-relative path of the file containing the `contents` array.
/// Workspace-root paths (starting with `/`) are resolved directly; relative paths are
/// resolved against the parent file's directory.
pub fn resolve_link_path(entry: &str, parent_rel: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_link_path_workspace_root_link() {
        let resolved = resolve_link_path(
            "[2026 Index](/Daily/2026/2026_index.md)",
            "Daily/daily_index.md",
        );
        assert_eq!(resolved, "Daily/2026/2026_index.md");
    }

    #[test]
    fn resolve_link_path_relative_link() {
        let resolved = resolve_link_path("2026/2026_index.md", "Daily/daily_index.md");
        assert_eq!(resolved, "Daily/2026/2026_index.md");
    }

    #[test]
    fn resolve_link_path_angle_bracket_root_link() {
        let resolved = resolve_link_path(
            "[2025 03 Entries](</Daily/2025/03/2025-03 entries.md>)",
            "Daily/2025/2025_index.md",
        );
        assert_eq!(resolved, "Daily/2025/03/2025-03 entries.md");
    }

    #[test]
    fn resolve_link_path_relative_entry_from_month_index() {
        let resolved = resolve_link_path("2025-09-19.md", "Daily/2025/09/09.md");
        assert_eq!(resolved, "Daily/2025/09/2025-09-19.md");
    }
}
