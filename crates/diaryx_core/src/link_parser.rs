//! Portable path link parsing and formatting for `part_of` and `contents` properties.
//!
//! This module provides utilities for working with file references in frontmatter that are:
//! - **Portable**: Work across Obsidian, Diaryx, and other markdown editors
//! - **Unambiguous**: Clear distinction between relative and workspace-root paths
//! - **Clickable**: Rendered as links in supporting editors
//! - **Self-documenting**: Include human-readable titles
//!
//! # Link Formats Supported (Read)
//!
//! | Format | Example | Interpretation |
//! |--------|---------|----------------|
//! | Markdown link (root) | `"[Title](/path/file.md)"` | Workspace-root absolute |
//! | Markdown link (relative) | `"[Title](../file.md)"` | Relative to current file |
//! | Plain root path | `/path/file.md` | Workspace-root absolute |
//! | Plain relative | `../file.md` | Relative to current file |
//! | Plain ambiguous | `path/file.md` | Assume relative (legacy) |
//!
//! # Link Format (Write)
//!
//! The write format is configurable via [`LinkFormat`]:
//! - `MarkdownRoot` (default): `"[Title](/workspace/root/path.md)"`
//! - `MarkdownRelative`: `"[Title](../relative/path.md)"`
//! - `PlainRelative`: `../relative/path.md`
//! - `PlainCanonical`: `workspace/root/path.md`
//!
//! # Internal CRDT Storage
//!
//! The CRDT layer stores canonical paths WITHOUT the `/` prefix:
//! ```text
//! Utility/utility_index.md
//! ```
//!
//! The `/` prefix and markdown link syntax are purely for frontmatter serialization.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The format to use when writing links to frontmatter.
///
/// This controls how `part_of` and `contents` paths are serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkFormat {
    /// Markdown link with workspace-root path: `[Title](/path/to/file.md)`
    ///
    /// This is the most portable and unambiguous format:
    /// - Clickable in Obsidian and other markdown editors
    /// - Unambiguous (always relative to workspace root)
    /// - Self-documenting with human-readable titles
    #[default]
    MarkdownRoot,

    /// Markdown link with relative path: `[Title](../relative/path.md)`
    ///
    /// Useful for compatibility with tools that don't understand root paths.
    MarkdownRelative,

    /// Plain relative path without markdown link syntax: `../relative/path.md`
    ///
    /// Legacy format for backwards compatibility.
    PlainRelative,

    /// Plain canonical path (workspace-relative): `path/to/file.md`
    ///
    /// Simple format without markdown link syntax or leading slash.
    PlainCanonical,
}

/// The type of path in a parsed link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    /// Path starts with `/` - workspace-root absolute path
    WorkspaceRoot,
    /// Path contains `../` or `./` - relative to current file
    Relative,
    /// Plain path like `folder/file.md` - ambiguous, assume relative (legacy)
    Ambiguous,
}

/// A parsed link from frontmatter.
///
/// This represents either a markdown link `[Title](path)` or a plain path string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    /// The display title, if present (from markdown link syntax)
    pub title: Option<String>,
    /// The path portion of the link
    pub path: String,
    /// The type of path (root, relative, or ambiguous)
    pub path_type: PathType,
}

impl ParsedLink {
    /// Create a new parsed link with just a path (no title).
    pub fn new(path: String, path_type: PathType) -> Self {
        Self {
            title: None,
            path,
            path_type,
        }
    }

    /// Create a new parsed link with a title.
    pub fn with_title(title: String, path: String, path_type: PathType) -> Self {
        Self {
            title: Some(title),
            path,
            path_type,
        }
    }
}

/// Parse a link value from frontmatter.
///
/// Handles multiple formats:
/// - Markdown links: `[Title](/path/file.md)` or `[Title](../file.md)`
/// - Plain paths with `/` prefix: `/path/file.md`
/// - Plain relative paths: `../file.md` or `./file.md`
/// - Plain ambiguous paths: `path/file.md`
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{parse_link, PathType};
///
/// // Markdown link with root path
/// let link = parse_link("[Utility Index](/Utility/utility_index.md)");
/// assert_eq!(link.title, Some("Utility Index".to_string()));
/// assert_eq!(link.path, "Utility/utility_index.md");
/// assert_eq!(link.path_type, PathType::WorkspaceRoot);
///
/// // Markdown link with relative path
/// let link = parse_link("[Parent](../index.md)");
/// assert_eq!(link.title, Some("Parent".to_string()));
/// assert_eq!(link.path, "../index.md");
/// assert_eq!(link.path_type, PathType::Relative);
///
/// // Plain root path
/// let link = parse_link("/Utility/file.md");
/// assert_eq!(link.title, None);
/// assert_eq!(link.path, "Utility/file.md");
/// assert_eq!(link.path_type, PathType::WorkspaceRoot);
///
/// // Plain relative path
/// let link = parse_link("../parent.md");
/// assert_eq!(link.path_type, PathType::Relative);
///
/// // Plain ambiguous path (legacy)
/// let link = parse_link("child.md");
/// assert_eq!(link.path_type, PathType::Ambiguous);
/// ```
pub fn parse_link(value: &str) -> ParsedLink {
    let value = value.trim();

    // Try to parse as markdown link: [Title](path)
    if let Some(parsed) = try_parse_markdown_link(value) {
        return parsed;
    }

    // Plain path - determine type
    let path_type = determine_path_type(value);

    // Strip leading `/` for workspace-root paths (CRDT stores without prefix)
    let path = if path_type == PathType::WorkspaceRoot {
        value.strip_prefix('/').unwrap_or(value).to_string()
    } else {
        value.to_string()
    };

    ParsedLink::new(path, path_type)
}

/// Try to parse a markdown link `[Title](path)`.
fn try_parse_markdown_link(value: &str) -> Option<ParsedLink> {
    // Must start with `[` and contain `](`
    if !value.starts_with('[') {
        return None;
    }

    // Find the closing bracket and opening paren
    let close_bracket = value.find(']')?;
    if !value[close_bracket..].starts_with("](") {
        return None;
    }

    // Find the closing paren
    let path_start = close_bracket + 2;
    let close_paren = value[path_start..].find(')')? + path_start;

    let title = value[1..close_bracket].to_string();
    let raw_path = value[path_start..close_paren].to_string();

    let path_type = determine_path_type(&raw_path);

    // Strip leading `/` for workspace-root paths
    let path = if path_type == PathType::WorkspaceRoot {
        raw_path.strip_prefix('/').unwrap_or(&raw_path).to_string()
    } else {
        raw_path
    };

    Some(ParsedLink::with_title(title, path, path_type))
}

/// Determine the path type from a raw path string.
fn determine_path_type(path: &str) -> PathType {
    if path.starts_with('/') {
        PathType::WorkspaceRoot
    } else if path.starts_with("../") || path.starts_with("./") || path == ".." || path == "." {
        PathType::Relative
    } else {
        PathType::Ambiguous
    }
}

/// Convert a parsed link to a canonical (workspace-relative) path.
///
/// - Workspace-root paths are already canonical (just stripped the `/`)
/// - Relative paths are resolved against the current file's directory
/// - Ambiguous paths are treated as relative (legacy behavior)
///
/// # Arguments
///
/// * `parsed` - The parsed link to convert
/// * `current_file_path` - The canonical path of the file containing this link
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{parse_link, to_canonical};
/// use std::path::Path;
///
/// // Workspace-root path - already canonical
/// let link = parse_link("[Title](/Utility/file.md)");
/// let canonical = to_canonical(&link, Path::new("Other/entry.md"));
/// assert_eq!(canonical, "Utility/file.md");
///
/// // Relative path - resolve against current file
/// let link = parse_link("../index.md");
/// let canonical = to_canonical(&link, Path::new("Folder/Sub/entry.md"));
/// assert_eq!(canonical, "Folder/index.md");
///
/// // Ambiguous path - treat as relative to current file's directory
/// let link = parse_link("child.md");
/// let canonical = to_canonical(&link, Path::new("Folder/index.md"));
/// assert_eq!(canonical, "Folder/child.md");
/// ```
pub fn to_canonical(parsed: &ParsedLink, current_file_path: &Path) -> String {
    match parsed.path_type {
        PathType::WorkspaceRoot => {
            // Already canonical (we stripped the `/` during parsing)
            parsed.path.clone()
        }
        PathType::Relative | PathType::Ambiguous => {
            // Resolve relative to current file's directory
            let file_dir = current_file_path.parent().unwrap_or(Path::new(""));
            let resolved = file_dir.join(&parsed.path);
            normalize_path(&resolved)
        }
    }
}

/// Normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &Path) -> String {
    use std::path::Component;

    let mut normalized: Vec<&str> = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible
                if !normalized.is_empty() && normalized.last() != Some(&"..") {
                    normalized.pop();
                } else {
                    // Can't go up further - this shouldn't happen for valid workspace paths
                    // but keep it for robustness
                    normalized.push("..");
                }
            }
            Component::CurDir => {
                // Skip `.` components
            }
            Component::Normal(s) => {
                if let Some(s) = s.to_str() {
                    normalized.push(s);
                }
            }
            _ => {}
        }
    }

    if normalized.is_empty() {
        String::new()
    } else {
        normalized.join("/")
    }
}

/// Format a canonical path as a markdown link for frontmatter (default format).
///
/// Creates a link in the format: `[Title](/canonical/path.md)`
///
/// This is a convenience function that uses `LinkFormat::MarkdownRoot`.
/// For other formats, use [`format_link_with_format`].
///
/// # Arguments
///
/// * `canonical_path` - The canonical (workspace-relative) path, without leading `/`
/// * `title` - The display title for the link
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::format_link;
///
/// let link = format_link("Utility/utility_index.md", "Utility Index");
/// assert_eq!(link, "[Utility Index](/Utility/utility_index.md)");
/// ```
pub fn format_link(canonical_path: &str, title: &str) -> String {
    format!("[{}](/{})", title, canonical_path)
}

/// Format a link based on the specified format.
///
/// # Arguments
///
/// * `canonical_path` - The canonical (workspace-relative) path of the target file
/// * `title` - The display title for the link
/// * `format` - The link format to use
/// * `from_canonical_path` - The canonical path of the file containing this link
///   (required for relative formats)
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::{format_link_with_format, LinkFormat};
///
/// // Markdown with root path (default)
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::MarkdownRoot,
///     "Other/source.md"
/// );
/// assert_eq!(link, "[Target](/Folder/target.md)");
///
/// // Markdown with relative path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::MarkdownRelative,
///     "Folder/source.md"
/// );
/// assert_eq!(link, "[Target](target.md)");
///
/// // Plain relative path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::PlainRelative,
///     "Folder/source.md"
/// );
/// assert_eq!(link, "target.md");
///
/// // Plain canonical path
/// let link = format_link_with_format(
///     "Folder/target.md",
///     "Target",
///     LinkFormat::PlainCanonical,
///     "Other/source.md"
/// );
/// assert_eq!(link, "Folder/target.md");
/// ```
pub fn format_link_with_format(
    canonical_path: &str,
    title: &str,
    format: LinkFormat,
    from_canonical_path: &str,
) -> String {
    match format {
        LinkFormat::MarkdownRoot => {
            format!("[{}](/{})", title, canonical_path)
        }
        LinkFormat::MarkdownRelative => {
            let relative = compute_relative_path(from_canonical_path, canonical_path);
            format!("[{}]({})", title, relative)
        }
        LinkFormat::PlainRelative => compute_relative_path(from_canonical_path, canonical_path),
        LinkFormat::PlainCanonical => canonical_path.to_string(),
    }
}

/// Compute a relative path from one file to another.
///
/// Both paths should be canonical (workspace-relative) paths.
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::compute_relative_path;
///
/// // Same directory
/// assert_eq!(compute_relative_path("Folder/a.md", "Folder/b.md"), "b.md");
///
/// // Child directory
/// assert_eq!(compute_relative_path("Folder/index.md", "Folder/Sub/child.md"), "Sub/child.md");
///
/// // Parent directory
/// assert_eq!(compute_relative_path("Folder/Sub/child.md", "Folder/index.md"), "../index.md");
///
/// // Sibling directory
/// assert_eq!(compute_relative_path("A/file.md", "B/file.md"), "../B/file.md");
///
/// // Root file from subdirectory
/// assert_eq!(compute_relative_path("Folder/file.md", "README.md"), "../README.md");
/// ```
pub fn compute_relative_path(from_path: &str, to_path: &str) -> String {
    let from_dir = Path::new(from_path).parent().unwrap_or(Path::new(""));
    let to_path = Path::new(to_path);

    // Split both paths into components
    let from_components: Vec<&str> = from_dir
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let to_components: Vec<&str> = to_path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    // Find common prefix length
    let common_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Build relative path: go up for each remaining from_component, then down to target
    let ups = from_components.len().saturating_sub(common_len);
    let downs = &to_components[common_len..];

    let mut result_parts: Vec<&str> = vec![".."; ups];
    for part in downs {
        result_parts.push(part);
    }

    if result_parts.is_empty() {
        // Same directory - just return the filename
        to_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(to_path.to_str().unwrap_or(""))
            .to_string()
    } else {
        result_parts.join("/")
    }
}

/// Generate a display title from a canonical path.
///
/// Uses the filename without extension, converting underscores/hyphens to spaces
/// and applying title case.
///
/// # Examples
///
/// ```
/// use diaryx_core::link_parser::path_to_title;
///
/// assert_eq!(path_to_title("utility_index.md"), "Utility Index");
/// assert_eq!(path_to_title("Folder/my-file.md"), "My File");
/// assert_eq!(path_to_title("2025.md"), "2025");
/// assert_eq!(path_to_title("README.md"), "README");
/// ```
pub fn path_to_title(path: &str) -> String {
    // Extract filename without extension
    let filename = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path);

    // Replace underscores and hyphens with spaces
    let spaced: String = filename
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();

    // Apply title case (capitalize first letter of each word)
    spaced
        .split_whitespace()
        .map(|word| {
            let mut chars: Vec<char> = word.chars().collect();
            if let Some(first) = chars.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            chars.into_iter().collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_link_with_root_path() {
        let link = parse_link("[Utility Index](/Utility/utility_index.md)");
        assert_eq!(link.title, Some("Utility Index".to_string()));
        assert_eq!(link.path, "Utility/utility_index.md");
        assert_eq!(link.path_type, PathType::WorkspaceRoot);
    }

    #[test]
    fn test_parse_markdown_link_with_relative_path() {
        let link = parse_link("[Parent](../index.md)");
        assert_eq!(link.title, Some("Parent".to_string()));
        assert_eq!(link.path, "../index.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_parse_markdown_link_with_ambiguous_path() {
        let link = parse_link("[Child](child.md)");
        assert_eq!(link.title, Some("Child".to_string()));
        assert_eq!(link.path, "child.md");
        assert_eq!(link.path_type, PathType::Ambiguous);
    }

    #[test]
    fn test_parse_plain_root_path() {
        let link = parse_link("/Utility/file.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "Utility/file.md");
        assert_eq!(link.path_type, PathType::WorkspaceRoot);
    }

    #[test]
    fn test_parse_plain_relative_path() {
        let link = parse_link("../parent.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "../parent.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_parse_plain_ambiguous_path() {
        let link = parse_link("child.md");
        assert_eq!(link.title, None);
        assert_eq!(link.path, "child.md");
        assert_eq!(link.path_type, PathType::Ambiguous);
    }

    #[test]
    fn test_parse_dotslash_relative() {
        let link = parse_link("./sibling.md");
        assert_eq!(link.path, "./sibling.md");
        assert_eq!(link.path_type, PathType::Relative);
    }

    #[test]
    fn test_to_canonical_workspace_root() {
        let link = parse_link("[Title](/Utility/file.md)");
        let canonical = to_canonical(&link, Path::new("Other/entry.md"));
        assert_eq!(canonical, "Utility/file.md");
    }

    #[test]
    fn test_to_canonical_relative_parent() {
        let link = parse_link("../index.md");
        let canonical = to_canonical(&link, Path::new("Folder/Sub/entry.md"));
        assert_eq!(canonical, "Folder/index.md");
    }

    #[test]
    fn test_to_canonical_relative_sibling() {
        let link = parse_link("./sibling.md");
        let canonical = to_canonical(&link, Path::new("Folder/entry.md"));
        assert_eq!(canonical, "Folder/sibling.md");
    }

    #[test]
    fn test_to_canonical_ambiguous() {
        let link = parse_link("child.md");
        let canonical = to_canonical(&link, Path::new("Folder/index.md"));
        assert_eq!(canonical, "Folder/child.md");
    }

    #[test]
    fn test_to_canonical_deep_relative() {
        let link = parse_link("../../root.md");
        let canonical = to_canonical(&link, Path::new("A/B/C/file.md"));
        assert_eq!(canonical, "A/root.md");
    }

    #[test]
    fn test_format_link() {
        let link = format_link("Utility/utility_index.md", "Utility Index");
        assert_eq!(link, "[Utility Index](/Utility/utility_index.md)");
    }

    #[test]
    fn test_format_link_root_file() {
        let link = format_link("README.md", "README");
        assert_eq!(link, "[README](/README.md)");
    }

    #[test]
    fn test_path_to_title_underscore() {
        assert_eq!(path_to_title("utility_index.md"), "Utility Index");
    }

    #[test]
    fn test_path_to_title_hyphen() {
        assert_eq!(path_to_title("my-file.md"), "My File");
    }

    #[test]
    fn test_path_to_title_with_path() {
        assert_eq!(path_to_title("Folder/sub_file.md"), "Sub File");
    }

    #[test]
    fn test_path_to_title_number() {
        assert_eq!(path_to_title("2025.md"), "2025");
    }

    #[test]
    fn test_path_to_title_uppercase() {
        assert_eq!(path_to_title("README.md"), "README");
    }

    #[test]
    fn test_roundtrip_link() {
        // Parse a markdown link, convert to canonical, format back
        let original = "[Daily Index](/Daily/daily_index.md)";
        let parsed = parse_link(original);
        let canonical = to_canonical(&parsed, Path::new("Other/file.md"));
        let title = parsed.title.unwrap_or_else(|| path_to_title(&canonical));
        let formatted = format_link(&canonical, &title);

        assert_eq!(formatted, "[Daily Index](/Daily/daily_index.md)");
    }

    #[test]
    fn test_roundtrip_relative_to_canonical_to_formatted() {
        // Start with relative path, convert to canonical, format as markdown link
        let relative = "../parent_index.md";
        let parsed = parse_link(relative);
        let canonical = to_canonical(&parsed, Path::new("Folder/child.md"));
        let title = path_to_title(&canonical);
        let formatted = format_link(&canonical, &title);

        assert_eq!(canonical, "parent_index.md");
        assert_eq!(formatted, "[Parent Index](/parent_index.md)");
    }

    // =========================================================================
    // compute_relative_path tests
    // =========================================================================

    #[test]
    fn test_compute_relative_path_same_directory() {
        assert_eq!(compute_relative_path("Folder/a.md", "Folder/b.md"), "b.md");
    }

    #[test]
    fn test_compute_relative_path_child_directory() {
        assert_eq!(
            compute_relative_path("Folder/index.md", "Folder/Sub/child.md"),
            "Sub/child.md"
        );
    }

    #[test]
    fn test_compute_relative_path_parent_directory() {
        assert_eq!(
            compute_relative_path("Folder/Sub/child.md", "Folder/index.md"),
            "../index.md"
        );
    }

    #[test]
    fn test_compute_relative_path_sibling_directory() {
        assert_eq!(
            compute_relative_path("A/file.md", "B/file.md"),
            "../B/file.md"
        );
    }

    #[test]
    fn test_compute_relative_path_root_from_subdir() {
        assert_eq!(
            compute_relative_path("Folder/file.md", "README.md"),
            "../README.md"
        );
    }

    #[test]
    fn test_compute_relative_path_deep_to_root() {
        assert_eq!(
            compute_relative_path("A/B/C/file.md", "README.md"),
            "../../../README.md"
        );
    }

    // =========================================================================
    // format_link_with_format tests
    // =========================================================================

    #[test]
    fn test_format_link_with_format_markdown_root() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRoot,
            "Other/source.md",
        );
        assert_eq!(link, "[Target](/Folder/target.md)");
    }

    #[test]
    fn test_format_link_with_format_markdown_relative_same_dir() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRelative,
            "Folder/source.md",
        );
        assert_eq!(link, "[Target](target.md)");
    }

    #[test]
    fn test_format_link_with_format_markdown_relative_parent() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::MarkdownRelative,
            "Folder/Sub/source.md",
        );
        assert_eq!(link, "[Target](../target.md)");
    }

    #[test]
    fn test_format_link_with_format_plain_relative() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::PlainRelative,
            "Folder/source.md",
        );
        assert_eq!(link, "target.md");
    }

    #[test]
    fn test_format_link_with_format_plain_canonical() {
        let link = format_link_with_format(
            "Folder/target.md",
            "Target",
            LinkFormat::PlainCanonical,
            "Other/source.md",
        );
        assert_eq!(link, "Folder/target.md");
    }

    // =========================================================================
    // LinkFormat tests
    // =========================================================================

    #[test]
    fn test_link_format_default() {
        assert_eq!(LinkFormat::default(), LinkFormat::MarkdownRoot);
    }

    #[test]
    fn test_link_format_serialize() {
        assert_eq!(
            serde_json::to_string(&LinkFormat::MarkdownRoot).unwrap(),
            "\"markdown_root\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::MarkdownRelative).unwrap(),
            "\"markdown_relative\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::PlainRelative).unwrap(),
            "\"plain_relative\""
        );
        assert_eq!(
            serde_json::to_string(&LinkFormat::PlainCanonical).unwrap(),
            "\"plain_canonical\""
        );
    }

    #[test]
    fn test_link_format_deserialize() {
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"markdown_root\"").unwrap(),
            LinkFormat::MarkdownRoot
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"markdown_relative\"").unwrap(),
            LinkFormat::MarkdownRelative
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"plain_relative\"").unwrap(),
            LinkFormat::PlainRelative
        );
        assert_eq!(
            serde_json::from_str::<LinkFormat>("\"plain_canonical\"").unwrap(),
            LinkFormat::PlainCanonical
        );
    }
}
