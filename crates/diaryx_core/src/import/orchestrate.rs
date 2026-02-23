//! Async orchestration for writing imported entries into a workspace.
//!
//! This module takes parsed [`ImportedEntry`] values and writes them to the
//! filesystem via [`AsyncFileSystem`], building the date-based folder hierarchy
//! with proper `part_of`/`contents` frontmatter links.
//!
//! The orchestration is shared between CLI, WASM, and Tauri frontends.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde_yaml::Value;

use crate::entry::slugify;
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::link_parser::format_link;
use crate::workspace::Workspace;

use super::{ImportResult, ImportedEntry};

/// Write imported entries into the workspace, building the date-based hierarchy.
///
/// Creates a folder structure like:
/// ```text
/// {folder}/
///   index.md               (root, contents → year indexes)
///   2024/
///     2024_index.md         (part_of → root, contents → month indexes)
///     01/
///       2024_01.md          (part_of → year, contents → entries)
///       2024-01-15-title.md (part_of → month)
/// ```
///
/// When `parent_path` is given, the folder is created under the parent entry's
/// directory and grafted into the parent's `contents`. Otherwise it's placed at
/// the workspace root.
pub async fn write_entries<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_root: &Path,
    folder: &str,
    entries: &[ImportedEntry],
    parent_path: Option<&str>,
) -> ImportResult {
    // Compute base directory and canonical prefix based on parent_path.
    let parent_dir = parent_path
        .and_then(|p| {
            Path::new(p)
                .parent()
                .map(|d| d.to_string_lossy().replace('\\', "/"))
        })
        .filter(|d| !d.is_empty());

    let (base_dir, canonical_prefix) = match &parent_dir {
        Some(dir) => (
            workspace_root.join(dir).join(folder),
            format!("{dir}/{folder}"),
        ),
        None => (workspace_root.join(folder), folder.to_string()),
    };
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
        attachment_count: 0,
    };

    if entries.is_empty() {
        return result;
    }

    // Track used filenames within each directory to handle collisions.
    let mut used_paths: HashSet<PathBuf> = HashSet::new();

    // Hierarchy tracking:
    //   year_canonical → { month_canonical → month_title }
    let mut year_to_months: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    //   month_canonical → list of entry links
    let mut month_to_entries: IndexMap<String, Vec<String>> = IndexMap::new();
    //   year_canonical → year_title
    let mut all_years: IndexMap<String, String> = IndexMap::new();

    for entry in entries {
        let (year, month, date_prefix) = date_components(entry);
        let slug = entry_slug(&entry.title);
        let filename = format!("{date_prefix}-{slug}.md");

        let month_dir = base_dir.join(&year).join(&month);
        let mut entry_path = month_dir.join(&filename);

        // Handle filename collisions.
        entry_path = deduplicate_path(entry_path, &used_paths);
        used_paths.insert(entry_path.clone());

        // Compute canonical paths for hierarchy tracking.
        let entry_canonical = canonical_path(workspace_root, &entry_path);
        let month_index_canonical = format!("{canonical_prefix}/{year}/{month}/{year}_{month}.md");
        let year_index_canonical = format!("{canonical_prefix}/{year}/{year}_index.md");

        // Track: root → years.
        all_years
            .entry(year_index_canonical.clone())
            .or_insert_with(|| year.clone());

        // Track: year → months.
        year_to_months
            .entry(year_index_canonical)
            .or_default()
            .entry(month_index_canonical.clone())
            .or_insert_with(|| format!("{year}-{month}"));

        // Track: month → entries.
        let entry_link = format_link(&entry_canonical, &entry.title);
        month_to_entries
            .entry(month_index_canonical.clone())
            .or_default()
            .push(entry_link);

        // Build entry markdown.
        let entry_content =
            format_entry(entry, &entry_path, workspace_root, &month_index_canonical);

        // Write entry file.
        if let Err(e) = fs.create_dir_all(&month_dir).await {
            result.errors.push(format!(
                "Failed to create directory {}: {e}",
                month_dir.display()
            ));
            result.skipped += 1;
            continue;
        }
        if let Err(e) = fs.write_file(&entry_path, &entry_content).await {
            result
                .errors
                .push(format!("Failed to write {}: {e}", entry_path.display()));
            result.skipped += 1;
            continue;
        }

        // Write attachments.
        if !entry.attachments.is_empty() {
            let entry_stem = entry_path.file_stem().unwrap().to_string_lossy();
            let attachments_dir = month_dir.join(format!("{entry_stem}/_attachments"));

            for att in &entry.attachments {
                let att_path = attachments_dir.join(&att.filename);
                if let Err(e) = fs.create_dir_all(&attachments_dir).await {
                    result
                        .errors
                        .push(format!("Failed to create attachment dir: {e}"));
                    continue;
                }
                if let Err(e) = fs.write_binary(&att_path, &att.data).await {
                    result
                        .errors
                        .push(format!("Failed to write attachment: {e}"));
                    continue;
                }
                result.attachment_count += 1;
            }
        }

        result.imported += 1;
    }

    // Write index hierarchy.
    write_index_hierarchy(
        fs,
        workspace_root,
        &base_dir,
        folder,
        &canonical_prefix,
        &all_years,
        &year_to_months,
        &month_to_entries,
    )
    .await;

    // Graft into the parent entry (or workspace root) so entries appear in the sidebar.
    graft_into_parent(fs, workspace_root, &canonical_prefix, folder, parent_path).await;

    result
}

/// Write the root, year, and month index files with `contents`/`part_of` links.
#[allow(clippy::too_many_arguments)]
async fn write_index_hierarchy<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_root: &Path,
    base_dir: &Path,
    folder: &str,
    canonical_prefix: &str,
    all_years: &IndexMap<String, String>,
    year_to_months: &IndexMap<String, IndexMap<String, String>>,
    month_to_entries: &IndexMap<String, Vec<String>>,
) {
    let root_index_canonical = format!("{canonical_prefix}/index.md");

    // Root index: {folder}/index.md
    let root_index = base_dir.join("index.md");
    if !fs.exists(&root_index).await {
        let mut sorted_years: Vec<(&String, &String)> = all_years.iter().collect();
        sorted_years.sort_by_key(|(canonical, _)| (*canonical).clone());

        let contents: Vec<Value> = sorted_years
            .iter()
            .map(|(canonical, title)| Value::String(format_link(canonical, title)))
            .collect();

        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String(capitalize(folder)));
        fm.insert("contents".to_string(), Value::Sequence(contents));

        let yaml = serde_yaml::to_string(&fm).unwrap_or_default();
        let content = format!("---\n{yaml}---\n");

        let _ = fs.create_dir_all(base_dir).await;
        let _ = fs.write_file(&root_index, &content).await;
    }

    // Year indexes.
    for (year_canonical, months) in year_to_months {
        let year_path = workspace_root.join(year_canonical);
        if !fs.exists(&year_path).await {
            let mut sorted_months: Vec<(&String, &String)> = months.iter().collect();
            sorted_months.sort_by_key(|(canonical, _)| (*canonical).clone());

            let contents: Vec<Value> = sorted_months
                .iter()
                .map(|(canonical, title)| Value::String(format_link(canonical, title)))
                .collect();

            let year_title = year_path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .replace("_index", "");

            let mut fm = IndexMap::new();
            fm.insert("title".to_string(), Value::String(year_title));
            fm.insert(
                "part_of".to_string(),
                Value::String(format_link(&root_index_canonical, &capitalize(folder))),
            );
            fm.insert("contents".to_string(), Value::Sequence(contents));

            let yaml = serde_yaml::to_string(&fm).unwrap_or_default();
            let content = format!("---\n{yaml}---\n");

            let year_dir = year_path.parent().unwrap();
            let _ = fs.create_dir_all(year_dir).await;
            let _ = fs.write_file(&year_path, &content).await;
        }
    }

    // Month indexes.
    for (month_canonical, entry_links) in month_to_entries {
        let month_path = workspace_root.join(month_canonical);
        if !fs.exists(&month_path).await {
            let month_title = month_path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .replace('_', "-");

            // Find parent year canonical.
            let year_canonical = year_to_months
                .keys()
                .find(|yk| {
                    year_to_months
                        .get(*yk)
                        .map(|m| m.contains_key(month_canonical))
                        .unwrap_or(false)
                })
                .cloned();

            let mut fm = IndexMap::new();
            fm.insert("title".to_string(), Value::String(month_title.clone()));

            if let Some(ref yc) = year_canonical {
                let year_title = workspace_root
                    .join(yc)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .replace("_index", "");
                fm.insert(
                    "part_of".to_string(),
                    Value::String(format_link(yc, &year_title)),
                );
            }

            let contents: Vec<Value> = entry_links
                .iter()
                .map(|link| Value::String(link.clone()))
                .collect();
            fm.insert("contents".to_string(), Value::Sequence(contents));

            let yaml = serde_yaml::to_string(&fm).unwrap_or_default();
            let content = format!("---\n{yaml}---\n");

            let month_dir = month_path.parent().unwrap();
            let _ = fs.create_dir_all(month_dir).await;
            let _ = fs.write_file(&month_path, &content).await;
        }
    }
}

/// Graft the import folder's root index into the parent entry or workspace root.
///
/// When `parent_path` is given, grafts into that entry. Otherwise falls back to
/// the workspace root index (the file with `contents` but no `part_of`).
///
/// 1. Add the import folder's index to the parent's `contents` if not already present.
/// 2. Set `part_of` on the import folder's index pointing back to the parent.
async fn graft_into_parent<FS: AsyncFileSystem>(
    fs: &FS,
    workspace_root: &Path,
    canonical_prefix: &str,
    folder: &str,
    parent_path: Option<&str>,
) {
    // Resolve the parent entry to graft into.
    let graft_target = if let Some(pp) = parent_path {
        let abs = workspace_root.join(pp);
        if fs.exists(&abs).await {
            abs
        } else {
            return;
        }
    } else {
        // Fall back to workspace root index.
        let ws = Workspace::new(fs);
        match ws.find_root_index_in_dir(workspace_root).await {
            Ok(Some(path)) => path,
            _ => return,
        }
    };

    let import_index_path = workspace_root.join(canonical_prefix).join("index.md");
    if !fs.exists(&import_index_path).await {
        return;
    }

    let import_index_canonical = format!("{canonical_prefix}/index.md");
    let import_title = capitalize(folder);

    // Step 1: Add to parent's contents.
    if let Ok(parent_content) = fs.read_to_string(&graft_target).await
        && let Ok(parsed) = frontmatter::parse_or_empty(&parent_content)
    {
        let mut fm = parsed.frontmatter;

        let already_listed = fm
            .get("contents")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter().any(|item| {
                    item.as_str()
                        .map(|s| s.contains(&import_index_canonical))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if !already_listed {
            let link = Value::String(format_link(&import_index_canonical, &import_title));
            match fm.get_mut("contents") {
                Some(Value::Sequence(seq)) => {
                    seq.push(link);
                }
                _ => {
                    fm.insert("contents".to_string(), Value::Sequence(vec![link]));
                }
            }

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = fs.write_file(&graft_target, &updated).await;
            }
        }
    }

    // Step 2: Set part_of on the import folder's root index.
    if let Ok(import_content) = fs.read_to_string(&import_index_path).await
        && let Ok(parsed) = frontmatter::parse_or_empty(&import_content)
    {
        let mut fm = parsed.frontmatter;

        if !fm.contains_key("part_of") {
            let parent_canonical = canonical_path(workspace_root, &graft_target);

            // Read the parent's title from its frontmatter if available.
            let parent_title = fs
                .read_to_string(&graft_target)
                .await
                .ok()
                .and_then(|c| frontmatter::parse_or_empty(&c).ok())
                .and_then(|p| {
                    p.frontmatter
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| fm_title_or_filename(&graft_target));

            fm.insert(
                "part_of".to_string(),
                Value::String(format_link(&parent_canonical, &parent_title)),
            );

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = fs.write_file(&import_index_path, &updated).await;
            }
        }
    }
}

// ── Helper functions ──────────────────────────────────────────────────

/// Format an ImportedEntry as a markdown string with frontmatter links.
fn format_entry(
    entry: &ImportedEntry,
    entry_path: &Path,
    workspace_root: &Path,
    month_index_canonical: &str,
) -> String {
    let mut fm = IndexMap::new();

    fm.insert("title".to_string(), Value::String(entry.title.clone()));

    // Add extra metadata (from, to, cc, tags, etc.).
    for (key, value) in &entry.metadata {
        fm.insert(key.clone(), value.clone());
    }

    if let Some(dt) = entry.date {
        fm.insert("date".to_string(), Value::String(dt.to_rfc3339()));
    }

    // part_of: link to month index.
    let (year, month, _) = date_components_from_datetime(entry.date);
    let month_title = format!("{year}-{month}");
    fm.insert(
        "part_of".to_string(),
        Value::String(format_link(month_index_canonical, &month_title)),
    );

    // Attachments list.
    if !entry.attachments.is_empty() {
        let entry_stem = entry_path.file_stem().unwrap().to_string_lossy();
        let entry_dir = entry_path.parent().unwrap();
        let att_list: Vec<Value> = entry
            .attachments
            .iter()
            .map(|a| {
                let att_abs = entry_dir.join(format!("{entry_stem}/_attachments/{}", a.filename));
                Value::String(canonical_path(workspace_root, &att_abs))
            })
            .collect();
        fm.insert("attachments".to_string(), Value::Sequence(att_list));
    }

    let yaml = serde_yaml::to_string(&fm).unwrap_or_default();

    // Resolve _attachments/ references in the body to include the entry stem,
    // so they point to the correct sibling directory.
    let body = if !entry.attachments.is_empty() {
        let entry_stem = entry_path.file_stem().unwrap().to_string_lossy();
        entry
            .body
            .replace("_attachments/", &format!("{entry_stem}/_attachments/"))
    } else {
        entry.body.clone()
    };

    format!("---\n{yaml}---\n{body}")
}

/// Compute the canonical path (workspace-relative, forward slashes).
fn canonical_path(workspace_root: &Path, abs_path: &Path) -> String {
    abs_path
        .strip_prefix(workspace_root)
        .unwrap_or(abs_path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Extract (year, month, date_prefix) from an entry's date or fall back to today.
fn date_components(entry: &ImportedEntry) -> (String, String, String) {
    date_components_from_datetime(entry.date)
}

fn date_components_from_datetime(
    dt: Option<chrono::DateTime<chrono::Utc>>,
) -> (String, String, String) {
    let dt = dt.unwrap_or_else(chrono::Utc::now);
    let year = dt.format("%Y").to_string();
    let month = dt.format("%m").to_string();
    let date_prefix = dt.format("%Y-%m-%d").to_string();
    (year, month, date_prefix)
}

/// Create a URL-safe slug from a title, or fall back to "untitled".
fn entry_slug(title: &str) -> String {
    let slug = slugify(title);
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

/// Deduplicate a file path by appending -2, -3, etc. if it's already taken.
fn deduplicate_path(path: PathBuf, used: &HashSet<PathBuf>) -> PathBuf {
    if !used.contains(&path) {
        return path;
    }

    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let parent = path.parent().unwrap_or(Path::new("."));

    let mut counter = 2;
    loop {
        let new_name = if ext.is_empty() {
            format!("{stem}-{counter}")
        } else {
            format!("{stem}-{counter}.{ext}")
        };
        let candidate = parent.join(new_name);
        if !used.contains(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().chain(c).collect(),
    }
}

/// Extract a title from a file path's stem, prettified.
fn fm_title_or_filename(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(crate::entry::prettify_filename)
        .unwrap_or_else(|| "Index".to_string())
}
