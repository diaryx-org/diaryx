//! Synchronous orchestration for writing imported entries into a workspace.
//!
//! Port of `diaryx_core::import::orchestrate::write_entries` using host bridge
//! calls instead of `AsyncFileSystem`.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde_yaml::Value;

use diaryx_core::entry::slugify;
use diaryx_core::frontmatter;
use diaryx_core::import::{ImportResult, ImportedEntry};
use diaryx_core::link_parser::format_link;

use crate::host_bridge;

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
pub fn write_entries(
    folder: &str,
    entries: &[ImportedEntry],
    parent_path: Option<&str>,
) -> ImportResult {
    // Compute base directory prefix and canonical prefix based on parent_path.
    let parent_dir = parent_path.and_then(|p| {
        let parent = p.rfind('/').map(|idx| &p[..idx]);
        parent.filter(|d| !d.is_empty()).map(|d| d.to_string())
    });

    let canonical_prefix = match &parent_dir {
        Some(dir) => format!("{dir}/{folder}"),
        None => folder.to_string(),
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
    let mut used_paths: HashSet<String> = HashSet::new();

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

        let month_dir = format!("{canonical_prefix}/{year}/{month}");
        let mut entry_path = format!("{month_dir}/{filename}");

        // Handle filename collisions.
        entry_path = deduplicate_path(entry_path, &used_paths);
        used_paths.insert(entry_path.clone());

        // Compute canonical paths for hierarchy tracking.
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
        let entry_link = format_link(&entry_path, &entry.title);
        month_to_entries
            .entry(month_index_canonical.clone())
            .or_default()
            .push(entry_link);

        // Build entry markdown.
        let entry_content = format_entry(entry, &entry_path, &month_index_canonical);

        // Write entry file.
        if let Err(e) = host_bridge::write_file(&entry_path, &entry_content) {
            result
                .errors
                .push(format!("Failed to write {entry_path}: {e}"));
            result.skipped += 1;
            continue;
        }

        // Write attachments.
        if !entry.attachments.is_empty() {
            let entry_stem = path_stem(&entry_path);
            let attachments_dir = format!("{month_dir}/{entry_stem}/_attachments");

            for att in &entry.attachments {
                let att_path = format!("{attachments_dir}/{}", att.filename);
                if let Err(e) = host_bridge::write_binary(&att_path, &att.data) {
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
        folder,
        &canonical_prefix,
        &all_years,
        &year_to_months,
        &month_to_entries,
    );

    // Graft into the parent entry (or workspace root) so entries appear in the sidebar.
    graft_into_parent(&canonical_prefix, folder, parent_path);

    result
}

/// Write the root, year, and month index files with `contents`/`part_of` links.
fn write_index_hierarchy(
    folder: &str,
    canonical_prefix: &str,
    all_years: &IndexMap<String, String>,
    year_to_months: &IndexMap<String, IndexMap<String, String>>,
    month_to_entries: &IndexMap<String, Vec<String>>,
) {
    let root_index_canonical = format!("{canonical_prefix}/index.md");

    // Root index: {folder}/index.md
    if !host_bridge::file_exists(&root_index_canonical).unwrap_or(false) {
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

        let _ = host_bridge::write_file(&root_index_canonical, &content);
    }

    // Year indexes.
    for (year_canonical, months) in year_to_months {
        if !host_bridge::file_exists(year_canonical).unwrap_or(false) {
            let mut sorted_months: Vec<(&String, &String)> = months.iter().collect();
            sorted_months.sort_by_key(|(canonical, _)| (*canonical).clone());

            let contents: Vec<Value> = sorted_months
                .iter()
                .map(|(canonical, title)| Value::String(format_link(canonical, title)))
                .collect();

            let year_title = path_stem(year_canonical).replace("_index", "");

            let mut fm = IndexMap::new();
            fm.insert("title".to_string(), Value::String(year_title));
            fm.insert(
                "part_of".to_string(),
                Value::String(format_link(&root_index_canonical, &capitalize(folder))),
            );
            fm.insert("contents".to_string(), Value::Sequence(contents));

            let yaml = serde_yaml::to_string(&fm).unwrap_or_default();
            let content = format!("---\n{yaml}---\n");

            let _ = host_bridge::write_file(year_canonical, &content);
        }
    }

    // Month indexes.
    for (month_canonical, entry_links) in month_to_entries {
        if !host_bridge::file_exists(month_canonical).unwrap_or(false) {
            let month_title = path_stem(month_canonical).replace('_', "-");

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
                let year_title = path_stem(yc).replace("_index", "");
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

            let _ = host_bridge::write_file(month_canonical, &content);
        }
    }
}

/// Graft the import folder's root index into the parent entry or workspace root.
fn graft_into_parent(canonical_prefix: &str, folder: &str, parent_path: Option<&str>) {
    // Resolve the parent entry to graft into.
    let graft_target = if let Some(pp) = parent_path {
        if host_bridge::file_exists(pp).unwrap_or(false) {
            pp.to_string()
        } else {
            return;
        }
    } else {
        // Fall back to workspace root index: find a file at root level with `contents`
        // but no `part_of`.
        match find_root_index() {
            Some(path) => path,
            None => return,
        }
    };

    let import_index_path = format!("{canonical_prefix}/index.md");
    if !host_bridge::file_exists(&import_index_path).unwrap_or(false) {
        return;
    }

    let import_title = capitalize(folder);

    // Step 1: Add to parent's contents.
    if let Ok(parent_content) = host_bridge::read_file(&graft_target)
        && let Ok(parsed) = frontmatter::parse_or_empty(&parent_content)
    {
        let mut fm = parsed.frontmatter;

        let already_listed = fm
            .get("contents")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter().any(|item| {
                    item.as_str()
                        .map(|s| s.contains(&import_index_path))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if !already_listed {
            let link = Value::String(format_link(&import_index_path, &import_title));
            match fm.get_mut("contents") {
                Some(Value::Sequence(seq)) => {
                    seq.push(link);
                }
                _ => {
                    fm.insert("contents".to_string(), Value::Sequence(vec![link]));
                }
            }

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = host_bridge::write_file(&graft_target, &updated);
            }
        }
    }

    // Step 2: Set part_of on the import folder's root index.
    if let Ok(import_content) = host_bridge::read_file(&import_index_path)
        && let Ok(parsed) = frontmatter::parse_or_empty(&import_content)
    {
        let mut fm = parsed.frontmatter;

        if !fm.contains_key("part_of") {
            // Read the parent's title from its frontmatter if available.
            let parent_title = host_bridge::read_file(&graft_target)
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
                Value::String(format_link(&graft_target, &parent_title)),
            );

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = host_bridge::write_file(&import_index_path, &updated);
            }
        }
    }
}

/// Find the workspace root index file (has `contents` but no `part_of`).
fn find_root_index() -> Option<String> {
    let files = host_bridge::list_files("").ok()?;
    // Look for root-level markdown files (no slashes in path, or just "index.md")
    for file in &files {
        // Only check root-level files
        if file.contains('/') {
            continue;
        }
        if !file.ends_with(".md") {
            continue;
        }
        if let Ok(content) = host_bridge::read_file(file) {
            if let Ok(parsed) = frontmatter::parse_or_empty(&content) {
                if parsed.frontmatter.contains_key("contents")
                    && !parsed.frontmatter.contains_key("part_of")
                {
                    return Some(file.clone());
                }
            }
        }
    }
    None
}

// ── Helper functions ──────────────────────────────────────────────────

/// Format an ImportedEntry as a markdown string with frontmatter links.
fn format_entry(entry: &ImportedEntry, entry_path: &str, month_index_canonical: &str) -> String {
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
        let entry_stem = path_stem(entry_path);
        let entry_dir = path_parent(entry_path);
        let att_list: Vec<Value> = entry
            .attachments
            .iter()
            .map(|a| {
                let att_path = if entry_dir.is_empty() {
                    format!("{entry_stem}/_attachments/{}", a.filename)
                } else {
                    format!("{entry_dir}/{entry_stem}/_attachments/{}", a.filename)
                };
                Value::String(att_path)
            })
            .collect();
        fm.insert("attachments".to_string(), Value::Sequence(att_list));
    }

    let yaml = serde_yaml::to_string(&fm).unwrap_or_default();

    // Resolve _attachments/ references in the body to include the entry stem.
    let body = if !entry.attachments.is_empty() {
        let entry_stem = path_stem(entry_path);
        entry
            .body
            .replace("_attachments/", &format!("{entry_stem}/_attachments/"))
    } else {
        entry.body.clone()
    };

    format!("---\n{yaml}---\n{body}")
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

/// Deduplicate a path string by appending -2, -3, etc. if it's already taken.
fn deduplicate_path(path: String, used: &HashSet<String>) -> String {
    if !used.contains(&path) {
        return path;
    }

    let stem = path_stem(&path);
    let ext = path_extension(&path);
    let parent = path_parent(&path);

    let mut counter = 2;
    loop {
        let new_name = if ext.is_empty() {
            format!("{stem}-{counter}")
        } else {
            format!("{stem}-{counter}.{ext}")
        };
        let candidate = if parent.is_empty() {
            new_name
        } else {
            format!("{parent}/{new_name}")
        };
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
fn fm_title_or_filename(path: &str) -> String {
    let stem = path_stem(path);
    diaryx_core::entry::prettify_filename(&stem)
}

/// Get the file stem (name without extension) from a path string.
fn path_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(idx) if idx > 0 => name[..idx].to_string(),
        _ => name.to_string(),
    }
}

/// Get the file extension from a path string.
fn path_extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(idx) if idx > 0 => name[idx + 1..].to_string(),
        _ => String::new(),
    }
}

/// Get the parent directory from a path string.
fn path_parent(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}
