//! Path utilities for calculating relative paths between files and directories.
//!
//! This module provides functions to compute relative paths, which is useful for
//! maintaining `part_of` and `contents` references in the workspace.

use std::path::{Component, Path, PathBuf};

/// Normalize a path by resolving `.` and `..` components without filesystem access.
///
/// This is necessary for web/WASM where the virtual filesystem doesn't handle
/// `..` in paths automatically.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::normalize_path;
/// use std::path::Path;
///
/// let path = Path::new("foo/bar/../baz.txt");
/// assert_eq!(normalize_path(path), Path::new("foo/baz.txt"));
/// ```
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible (handle ..)
                if !normalized.is_empty()
                    && !matches!(normalized.last(), Some(Component::ParentDir))
                {
                    normalized.pop();
                } else {
                    // Can't go up further, keep the ..
                    normalized.push(component);
                }
            }
            Component::CurDir => {
                // Skip . components
            }
            _ => {
                normalized.push(component);
            }
        }
    }

    normalized.iter().collect()
}

/// Normalize a workspace-relative sync path to canonical form.
///
/// Rules:
/// - Convert backslashes to forward slashes
/// - Strip leading `./` prefixes
/// - Strip leading `/` prefixes
///
/// This is used for CRDT/sync key normalization so path aliases like
/// `README.md`, `./README.md`, and `/README.md` are treated as the same file.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::normalize_sync_path;
///
/// assert_eq!(normalize_sync_path("./notes\\hello.md"), "notes/hello.md");
/// assert_eq!(normalize_sync_path("/notes/hello.md"), "notes/hello.md");
/// ```
pub fn normalize_sync_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

/// Strip a workspace root prefix from a path string, returning a workspace-relative path.
///
/// Handles both normal absolute paths and the corrupted form where the leading `/`
/// has already been stripped (for example `Users/alice/workspace/README.md`).
///
/// Returns `None` when the path does not belong to the workspace root.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::strip_workspace_root_prefix;
/// use std::path::Path;
///
/// let root = Path::new("/Users/alice/workspace");
/// assert_eq!(
///     strip_workspace_root_prefix("/Users/alice/workspace/notes/day.md", root),
///     Some("notes/day.md".to_string())
/// );
/// assert_eq!(
///     strip_workspace_root_prefix("Users/alice/workspace/notes/day.md", root),
///     Some("notes/day.md".to_string())
/// );
/// ```
pub fn strip_workspace_root_prefix(path: &str, workspace_root: &Path) -> Option<String> {
    if path.is_empty() {
        return Some(String::new());
    }

    let path_obj = Path::new(path);
    if let Ok(relative) = path_obj.strip_prefix(workspace_root) {
        return Some(relative.to_string_lossy().to_string());
    }

    let path_norm = path.replace('\\', "/");
    let root_norm = workspace_root.to_string_lossy().replace('\\', "/");
    let root_trimmed = root_norm.trim_end_matches('/');

    if root_trimmed.is_empty() {
        return None;
    }

    // Support both "/Users/..." and "Users/..." root prefixes.
    let mut candidates = Vec::with_capacity(3);
    candidates.push(root_trimmed.to_string());
    let root_without_leading = root_trimmed.trim_start_matches('/');
    if root_without_leading != root_trimmed {
        candidates.push(root_without_leading.to_string());
    }
    if !root_trimmed.starts_with('/') {
        candidates.push(format!("/{}", root_trimmed));
    }

    for candidate in candidates {
        if path_norm == candidate {
            return Some(String::new());
        }

        let prefix = format!("{candidate}/");
        if let Some(relative) = path_norm.strip_prefix(&prefix) {
            return Some(relative.to_string());
        }
    }

    None
}

/// Compute a relative path from a base directory to a target file.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::relative_path_from_dir_to_target;
/// use std::path::Path;
///
/// // From workspace/ to workspace/Daily/daily_index.md => Daily/daily_index.md
/// let base = Path::new("workspace");
/// let target = Path::new("workspace/Daily/daily_index.md");
/// let rel = relative_path_from_dir_to_target(base, target);
/// assert_eq!(rel, "Daily/daily_index.md");
/// ```
pub fn relative_path_from_dir_to_target(base_dir: &Path, target_path: &Path) -> String {
    let base_components: Vec<_> = base_dir.components().collect();
    let target_components: Vec<_> = target_path.components().collect();

    let mut common = 0usize;
    while common < base_components.len()
        && common < target_components.len()
        && base_components[common] == target_components[common]
    {
        common += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_components.len() {
        parts.push("..".to_string());
    }

    for comp in target_components.iter().skip(common) {
        parts.push(comp.as_os_str().to_string_lossy().to_string());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Compute a relative path from a source file's location to a target file.
///
/// This is useful for computing `part_of` values - the path from an entry to its parent index.
///
/// # Example
/// ```
/// use diaryx_core::path_utils::relative_path_from_file_to_target;
/// use std::path::Path;
///
/// // From a/b/note.md to a/index.md => ../index.md
/// let from = Path::new("a/b/note.md");
/// let to = Path::new("a/index.md");
/// let rel = relative_path_from_file_to_target(from, to);
/// assert_eq!(rel, "../index.md");
/// ```
pub fn relative_path_from_file_to_target(from_file: &Path, to_target: &Path) -> String {
    // We want relative from the file's directory
    let from_dir = from_file.parent().unwrap_or_else(|| Path::new(""));

    relative_path_from_dir_to_target(from_dir, to_target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_relative_path_same_dir() {
        let base = Path::new("workspace");
        let target = Path::new("workspace/file.md");
        assert_eq!(relative_path_from_dir_to_target(base, target), "file.md");
    }

    #[test]
    fn test_relative_path_nested() {
        let base = Path::new("workspace");
        let target = Path::new("workspace/Daily/2025/01/entry.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "Daily/2025/01/entry.md"
        );
    }

    #[test]
    fn test_relative_path_parent() {
        let base = Path::new("workspace/Daily");
        let target = Path::new("workspace/README.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "../README.md"
        );
    }

    #[test]
    fn test_relative_path_sibling() {
        let base = Path::new("workspace/Daily");
        let target = Path::new("workspace/Projects/index.md");
        assert_eq!(
            relative_path_from_dir_to_target(base, target),
            "../Projects/index.md"
        );
    }

    #[test]
    fn test_file_to_target_same_dir() {
        let from = Path::new("workspace/note.md");
        let to = Path::new("workspace/index.md");
        assert_eq!(relative_path_from_file_to_target(from, to), "index.md");
    }

    #[test]
    fn test_file_to_target_parent() {
        let from = Path::new("workspace/subdir/note.md");
        let to = Path::new("workspace/index.md");
        assert_eq!(relative_path_from_file_to_target(from, to), "../index.md");
    }

    #[test]
    fn test_file_to_target_nested() {
        let from = Path::new("a/b/c/note.md");
        let to = Path::new("a/index.md");
        assert_eq!(
            relative_path_from_file_to_target(from, to),
            "../../index.md"
        );
    }

    #[test]
    fn test_normalize_sync_path() {
        assert_eq!(normalize_sync_path("README.md"), "README.md");
        assert_eq!(normalize_sync_path("./README.md"), "README.md");
        assert_eq!(normalize_sync_path("/README.md"), "README.md");
        assert_eq!(normalize_sync_path(".//nested\\file.md"), "nested/file.md");
    }

    #[test]
    fn test_strip_workspace_root_prefix_absolute_path() {
        let root = Path::new("/Users/test/workspace");
        let result = strip_workspace_root_prefix("/Users/test/workspace/notes/day.md", root);
        assert_eq!(result.as_deref(), Some("notes/day.md"));
    }

    #[test]
    fn test_strip_workspace_root_prefix_corrupted_absolute_without_leading_slash() {
        let root = Path::new("/Users/test/workspace");
        let result = strip_workspace_root_prefix("Users/test/workspace/notes/day.md", root);
        assert_eq!(result.as_deref(), Some("notes/day.md"));
    }

    #[test]
    fn test_strip_workspace_root_prefix_returns_none_for_non_workspace_path() {
        let root = Path::new("/Users/test/workspace");
        let result = strip_workspace_root_prefix("/Users/test/other/notes/day.md", root);
        assert!(result.is_none());
    }
}
