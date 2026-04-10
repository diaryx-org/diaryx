//! Pure helpers shared across the validator and fixer.
//!
//! Nothing in this module touches async filesystem state except
//! [`find_index_in_directory`], which is kept here so both
//! [`super::validator`] and [`super::fixer`] can call it without cross-linking.

use std::path::{Component, Path, PathBuf};

use crate::fs::AsyncFileSystem;
use crate::link_parser::{self, LinkFormat};
use crate::path_utils::normalize_sync_path;
use crate::workspace::Workspace;

use super::types::{ValidationResult, ValidationWarning};

/// Normalize a path by removing `.` and `..` components without filesystem access.
/// This is more reliable than `canonicalize()` which can fail on WASM or with symlinks.
pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {
                // Skip `.` components
            }
            _ => {
                normalized.push(component);
            }
        }
    }
    normalized
}

/// Check if a path is clearly non-portable (machine-specific absolute path).
///
/// This function uses heuristics to detect paths that are clearly specific to
/// a particular machine and will never work when synced to other environments.
/// It does not require filesystem access, making it WASM-safe.
pub(super) fn is_clearly_non_portable_path(value: &str) -> bool {
    let path = Path::new(value);
    let value_lower = value.to_lowercase();

    // All patterns must be lowercase since we compare against value_lower.
    let machine_specific_patterns = [
        "/users/", // macOS
        "/home/",  // Linux
        "/root/",  // Linux root
        "/var/",   // Unix system directories
        "/tmp/",
        "/opt/",
        "/usr/",
        "c:\\users\\", // Windows (backslash)
        "c:/users/",   // Windows (forward slash)
        "d:\\users\\",
        "d:/users/",
        "c:\\program files",
        "c:/program files",
        "c:\\windows",
        "c:/windows",
        "\\\\", // UNC paths
    ];

    for pattern in machine_specific_patterns {
        if value_lower.starts_with(pattern) {
            return true;
        }
    }

    let is_windows_absolute = value.len() >= 2
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && (value.chars().nth(1) == Some(':'));

    let is_absolute = path.is_absolute() || is_windows_absolute;

    if !is_absolute {
        return false;
    }

    // Deep absolute paths (>4 components) are likely machine-specific.
    path.components().count() > 4
}

/// Check if a path string looks like an absolute path (cross-platform, WASM-safe).
pub(super) fn looks_like_absolute_path(value: &str) -> bool {
    if value.starts_with('/') {
        return true;
    }
    if value.len() >= 2
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && value.chars().nth(1) == Some(':')
    {
        return true;
    }
    if value.starts_with("\\\\") {
        return true;
    }
    false
}

/// Compute a suggested portable path without using filesystem operations.
pub(super) fn compute_suggested_portable_path(value: &str, base_dir: &Path) -> String {
    let path = Path::new(value);

    if looks_like_absolute_path(value) {
        let filename = match path.file_name() {
            Some(f) => f.to_string_lossy().to_string(),
            None => return value.to_string(),
        };

        let target_dirs: Vec<&std::ffi::OsStr> = path
            .parent()
            .map(|p| p.iter().collect())
            .unwrap_or_default();

        let source_dirs: Vec<&std::ffi::OsStr> = base_dir.iter().collect();

        for (target_idx, target_dir) in target_dirs.iter().enumerate().rev() {
            let target_str = target_dir.to_string_lossy();
            if target_str == "/" || target_str == "\\" || target_str.ends_with(':') {
                continue;
            }

            for (source_idx, source_dir) in source_dirs.iter().enumerate().rev() {
                let source_str = source_dir.to_string_lossy();
                if source_str == "/" || source_str == "\\" || source_str.ends_with(':') {
                    continue;
                }

                if target_dir == source_dir {
                    let target_suffix = &target_dirs[target_idx..];
                    let source_suffix = &source_dirs[source_idx..];

                    let matching_count = target_suffix
                        .iter()
                        .zip(source_suffix.iter())
                        .take_while(|(t, s)| t == s)
                        .count();

                    if matching_count > 0 {
                        let levels_up = source_suffix.len() - matching_count;
                        let extra_dirs = &target_suffix[matching_count..];

                        let mut result = "../".repeat(levels_up);
                        for dir in extra_dirs {
                            result.push_str(&dir.to_string_lossy());
                            result.push('/');
                        }
                        result.push_str(&filename);
                        return result;
                    }
                }
            }
        }

        return filename;
    }

    let target_path = base_dir.join(value);
    let normalized = normalize_path(&target_path);

    pathdiff::diff_paths(&normalized, base_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| value.to_string())
}

/// Check if a path reference is non-portable (contains `.` or `..` components).
///
/// NOTE: Absolute paths are handled separately by `is_clearly_non_portable_path`.
/// This function only handles relative paths with `.` or `..` components.
pub(super) fn check_non_portable_path(
    file: &Path,
    property: &str,
    value: &str,
    base_dir: &Path,
) -> Option<ValidationWarning> {
    let path = Path::new(value);

    if path.is_absolute() {
        return None;
    }

    let has_dot_component = path.components().any(|c| {
        matches!(
            c,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    });

    if has_dot_component {
        let suggested = compute_suggested_portable_path(value, base_dir);

        if suggested != value {
            return Some(ValidationWarning::NonPortablePath {
                file: file.to_path_buf(),
                property: property.to_string(),
                value: value.to_string(),
                suggested,
            });
        }
    }

    None
}

pub(super) fn workspace_relative_canonical_path(path: &Path, workspace_root: &Path) -> String {
    let relative = path.strip_prefix(workspace_root).unwrap_or(path);
    normalize_sync_path(&relative.to_string_lossy())
}

pub(super) fn expected_self_link(
    file_canonical: &str,
    title: Option<&str>,
    link_format: Option<LinkFormat>,
) -> String {
    let resolved_title = title
        .map(ToString::to_string)
        .unwrap_or_else(|| link_parser::path_to_title(file_canonical));
    link_parser::format_link_with_format(
        file_canonical,
        &resolved_title,
        link_format.unwrap_or_default(),
        file_canonical,
    )
}

pub(super) fn canonicalize_link_value(
    raw_value: &str,
    file_canonical: &str,
    link_format: Option<LinkFormat>,
) -> String {
    let parsed = link_parser::parse_link(raw_value);
    link_parser::to_canonical_with_link_format(&parsed, Path::new(file_canonical), link_format)
}

pub(super) fn list_contains_canonical_link(
    values: &[String],
    target_canonical: &str,
    file_canonical: &str,
    link_format: Option<LinkFormat>,
) -> bool {
    values.iter().any(|value| {
        canonicalize_link_value(value, file_canonical, link_format) == target_canonical
    })
}

/// Emit `DuplicateListEntry` warnings for every link-bearing list on an
/// index file. Covers `contents`, `attachments`, `links`, `link_of`, and
/// `attachment_of`. Duplicate detection uses canonical-link equivalence so
/// `[Foo](./foo.md)` and `foo.md` collapse together.
pub(super) fn check_duplicate_lists(
    result: &mut ValidationResult,
    file: &Path,
    frontmatter: &crate::workspace::IndexFrontmatter,
    file_canonical: &str,
    link_format: Option<LinkFormat>,
) {
    let properties: &[(&str, &[String])] = &[
        ("contents", frontmatter.contents_list()),
        ("attachments", frontmatter.attachments_list()),
        ("links", frontmatter.links_list()),
        ("link_of", frontmatter.link_of_list()),
        ("attachment_of", frontmatter.attachment_of_list()),
    ];
    for (property, values) in properties {
        if values.len() < 2 {
            continue;
        }
        push_duplicate_list_warnings(result, file, property, values, file_canonical, link_format);
    }
}

/// Emit `DuplicateListEntry` warnings for any entries in a link-bearing list
/// that collapse to the same canonical path. The first occurrence is kept as
/// the warning's `value` so the UI shows what the user actually wrote.
fn push_duplicate_list_warnings(
    result: &mut ValidationResult,
    file: &Path,
    property: &str,
    values: &[String],
    file_canonical: &str,
    link_format: Option<LinkFormat>,
) {
    // IndexMap preserves insertion order so warnings are emitted in list order.
    let mut groups: indexmap::IndexMap<String, (String, usize)> = indexmap::IndexMap::new();
    for raw in values {
        let canonical = canonicalize_link_value(raw, file_canonical, link_format);
        groups
            .entry(canonical)
            .and_modify(|(_, count)| *count += 1)
            .or_insert_with(|| (raw.clone(), 1));
    }
    for (_, (first_value, count)) in groups {
        if count > 1 {
            result.warnings.push(ValidationWarning::DuplicateListEntry {
                file: file.to_path_buf(),
                property: property.to_string(),
                value: first_value,
                count,
            });
        }
    }
}

/// Find a single index file in a directory. Returns `Some` iff exactly one
/// index is found. A file is an index iff its frontmatter has a `contents`
/// property (same rule as `IndexFrontmatter::is_index`). Filename is never
/// consulted. Excludes the file specified in `exclude` from the search.
pub(super) async fn find_index_in_directory<FS: AsyncFileSystem>(
    ws: &Workspace<FS>,
    dir: &Path,
    exclude: Option<&Path>,
) -> Option<PathBuf> {
    let mut indexes = Vec::new();

    if let Ok(entries) = ws.fs_ref().list_files(dir).await {
        for entry_path in entries {
            if let Some(excl) = exclude
                && entry_path == excl
            {
                continue;
            }

            if entry_path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            if ws.fs_ref().is_dir(&entry_path).await {
                continue;
            }
            if let Ok(index) = ws.parse_index(&entry_path).await
                && index.frontmatter.is_index()
            {
                indexes.push(entry_path);
            }
        }
    }

    if indexes.len() == 1 {
        indexes.into_iter().next()
    } else {
        None
    }
}
