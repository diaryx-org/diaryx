//! Synchronous orchestration for writing imported entries into a workspace.
//!
//! Uses host bridge calls instead of `AsyncFileSystem`.

use std::{collections::HashSet, path::Path};

use diaryx_core::yaml_value::YamlValue as Value;
use indexmap::IndexMap;

use diaryx_core::entry::slugify;
use diaryx_core::frontmatter;
use diaryx_core::link_parser::{
    LinkFormat, PathType, compute_relative_path, format_link_with_format, parse_link, to_canonical,
};

use crate::types::{ImportResult, ImportedEntry};
use diaryx_plugin_sdk::host;

pub struct ImportWriter {
    folder: String,
    canonical_prefix: String,
    parent_path: Option<String>,
    link_format: LinkFormat,
    result: ImportResult,
    used_paths: HashSet<String>,
    year_to_months: IndexMap<String, IndexMap<String, String>>,
    month_to_entries: IndexMap<String, Vec<String>>,
    all_years: IndexMap<String, String>,
}

impl ImportWriter {
    pub fn new(folder: &str, parent_path: Option<&str>) -> Self {
        let parent_path = normalize_optional_path(parent_path);
        let parent_dir = parent_path.and_then(|p| {
            let parent = p.rfind('/').map(|idx| &p[..idx]);
            parent.filter(|d| !d.is_empty()).map(|d| d.to_string())
        });
        let link_format = workspace_link_format(parent_path);

        let canonical_prefix = match &parent_dir {
            Some(dir) => format!("{dir}/{folder}"),
            None => folder.to_string(),
        };

        Self {
            folder: folder.to_string(),
            canonical_prefix,
            parent_path: parent_path.map(str::to_string),
            link_format,
            result: ImportResult {
                imported: 0,
                skipped: 0,
                errors: Vec::new(),
                attachment_count: 0,
            },
            used_paths: HashSet::new(),
            year_to_months: IndexMap::new(),
            month_to_entries: IndexMap::new(),
            all_years: IndexMap::new(),
        }
    }

    pub fn write_entry(&mut self, entry: &ImportedEntry) {
        let (year, month, date_prefix) = date_components(entry);
        let slug = entry_slug(&entry.title);
        let filename = format!("{date_prefix}-{slug}.md");

        let month_dir = format!("{}/{year}/{month}", self.canonical_prefix);
        let mut entry_path = format!("{month_dir}/{filename}");

        entry_path = deduplicate_path(entry_path, &self.used_paths);
        self.used_paths.insert(entry_path.clone());

        let month_index_canonical =
            format!("{}/{year}/{month}/{year}_{month}.md", self.canonical_prefix);
        let year_index_canonical = format!("{}/{year}/{year}_index.md", self.canonical_prefix);

        self.all_years
            .entry(year_index_canonical.clone())
            .or_insert_with(|| year.clone());

        self.year_to_months
            .entry(year_index_canonical)
            .or_default()
            .entry(month_index_canonical.clone())
            .or_insert_with(|| format!("{year}-{month}"));

        let entry_link = format_link_with_format(
            &entry_path,
            &entry.title,
            self.link_format,
            &month_index_canonical,
        );
        self.month_to_entries
            .entry(month_index_canonical.clone())
            .or_default()
            .push(entry_link);

        let entry_content =
            format_entry(entry, &entry_path, &month_index_canonical, self.link_format);

        if let Err(e) = host::fs::write_file(&entry_path, &entry_content) {
            self.result
                .errors
                .push(format!("Failed to write {entry_path}: {e}"));
            self.result.skipped += 1;
            return;
        }

        if !entry.attachments.is_empty() {
            let entry_stem = path_stem(&entry_path);
            let attachments_dir = format!("{month_dir}/{entry_stem}/_attachments");

            for att in &entry.attachments {
                let att_path = format!("{attachments_dir}/{}", att.filename);
                if let Err(e) = host::fs::write_binary(&att_path, &att.data) {
                    self.result
                        .errors
                        .push(format!("Failed to write attachment: {e}"));
                    continue;
                }
                self.result.attachment_count += 1;

                // Create attachment note.
                let note_path = format!("{att_path}.md");
                let note_content = format_attachment_note(
                    &att.filename,
                    &att_path,
                    &note_path,
                    &entry_path,
                    &entry.title,
                    self.link_format,
                );
                if let Err(e) = host::fs::write_file(&note_path, &note_content) {
                    self.result
                        .errors
                        .push(format!("Failed to write attachment note: {e}"));
                }
            }
        }

        self.result.imported += 1;
    }

    pub fn finish(self) -> ImportResult {
        if self.result.imported == 0 {
            return self.result;
        }

        write_index_hierarchy(
            &self.folder,
            &self.canonical_prefix,
            self.link_format,
            &self.all_years,
            &self.year_to_months,
            &self.month_to_entries,
        );

        graft_into_parent(
            &self.canonical_prefix,
            &self.folder,
            self.parent_path.as_deref(),
            self.link_format,
        );

        self.result
    }
}

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
    let mut writer = ImportWriter::new(folder, parent_path);
    for entry in entries {
        writer.write_entry(entry);
    }
    writer.finish()
}

/// Write the root, year, and month index files with `contents`/`part_of` links.
fn write_index_hierarchy(
    folder: &str,
    canonical_prefix: &str,
    link_format: LinkFormat,
    all_years: &IndexMap<String, String>,
    year_to_months: &IndexMap<String, IndexMap<String, String>>,
    month_to_entries: &IndexMap<String, Vec<String>>,
) {
    let root_index_canonical = format!("{canonical_prefix}/index.md");

    // Root index: {folder}/index.md
    if !host::fs::file_exists(&root_index_canonical).unwrap_or(false) {
        let mut sorted_years: Vec<(&String, &String)> = all_years.iter().collect();
        sorted_years.sort_by_key(|(canonical, _)| (*canonical).clone());

        let contents: Vec<Value> = sorted_years
            .iter()
            .map(|(canonical, title)| {
                Value::String(format_link_with_format(
                    canonical,
                    title,
                    link_format,
                    &root_index_canonical,
                ))
            })
            .collect();

        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String(capitalize(folder)));
        fm.insert(
            "link".to_string(),
            Value::String(format_link_with_format(
                &root_index_canonical,
                &capitalize(folder),
                link_format,
                &root_index_canonical,
            )),
        );
        fm.insert("contents".to_string(), Value::Sequence(contents));

        let yaml = serde_yaml_ng::to_string(&fm).unwrap_or_default();
        let content = format!("---\n{yaml}---\n");

        let _ = host::fs::write_file(&root_index_canonical, &content);
    }

    // Year indexes.
    for (year_canonical, months) in year_to_months {
        if !host::fs::file_exists(year_canonical).unwrap_or(false) {
            let mut sorted_months: Vec<(&String, &String)> = months.iter().collect();
            sorted_months.sort_by_key(|(canonical, _)| (*canonical).clone());

            let contents: Vec<Value> = sorted_months
                .iter()
                .map(|(canonical, title)| {
                    Value::String(format_link_with_format(
                        canonical,
                        title,
                        link_format,
                        year_canonical,
                    ))
                })
                .collect();

            let year_title = path_stem(year_canonical).replace("_index", "");

            let mut fm = IndexMap::new();
            fm.insert("title".to_string(), Value::String(year_title.clone()));
            fm.insert(
                "link".to_string(),
                Value::String(format_link_with_format(
                    year_canonical,
                    &year_title,
                    link_format,
                    year_canonical,
                )),
            );
            fm.insert(
                "part_of".to_string(),
                Value::String(format_link_with_format(
                    &root_index_canonical,
                    &capitalize(folder),
                    link_format,
                    year_canonical,
                )),
            );
            fm.insert("contents".to_string(), Value::Sequence(contents));

            let yaml = serde_yaml_ng::to_string(&fm).unwrap_or_default();
            let content = format!("---\n{yaml}---\n");

            let _ = host::fs::write_file(year_canonical, &content);
        }
    }

    // Month indexes.
    for (month_canonical, entry_links) in month_to_entries {
        if !host::fs::file_exists(month_canonical).unwrap_or(false) {
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
            fm.insert(
                "link".to_string(),
                Value::String(format_link_with_format(
                    month_canonical,
                    &month_title,
                    link_format,
                    month_canonical,
                )),
            );

            if let Some(ref yc) = year_canonical {
                let year_title = path_stem(yc).replace("_index", "");
                fm.insert(
                    "part_of".to_string(),
                    Value::String(format_link_with_format(
                        yc,
                        &year_title,
                        link_format,
                        month_canonical,
                    )),
                );
            }

            let contents: Vec<Value> = entry_links
                .iter()
                .map(|link| Value::String(link.clone()))
                .collect();
            fm.insert("contents".to_string(), Value::Sequence(contents));

            let yaml = serde_yaml_ng::to_string(&fm).unwrap_or_default();
            let content = format!("---\n{yaml}---\n");

            let _ = host::fs::write_file(month_canonical, &content);
        }
    }
}

/// Graft the import folder's root index into the parent entry or workspace root.
fn graft_into_parent(
    canonical_prefix: &str,
    folder: &str,
    parent_path: Option<&str>,
    link_format: LinkFormat,
) {
    let parent_path = normalize_optional_path(parent_path);
    // Resolve the parent entry to graft into.
    let graft_target = if let Some(pp) = parent_path {
        if host::fs::file_exists(pp).unwrap_or(false) {
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
    if !host::fs::file_exists(&import_index_path).unwrap_or(false) {
        return;
    }

    let import_title = capitalize(folder);

    // Step 1: Add to parent's contents.
    if let Ok(parent_content) = host::fs::read_file(&graft_target)
        && let Ok(parsed) = frontmatter::parse_or_empty(&parent_content)
    {
        let mut fm = parsed.frontmatter;

        let already_listed = fm
            .get("contents")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter().any(|item| {
                    item.as_str()
                        .map(|s| {
                            let parsed = parse_link(s);
                            to_canonical(&parsed, Path::new(&graft_target)) == import_index_path
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if !already_listed {
            let link = Value::String(format_link_with_format(
                &import_index_path,
                &import_title,
                link_format,
                &graft_target,
            ));
            match fm.get_mut("contents") {
                Some(Value::Sequence(seq)) => {
                    seq.push(link);
                }
                _ => {
                    fm.insert("contents".to_string(), Value::Sequence(vec![link]));
                }
            }

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = host::fs::write_file(&graft_target, &updated);
            }
        }
    }

    // Step 2: Set part_of on the import folder's root index.
    if let Ok(import_content) = host::fs::read_file(&import_index_path)
        && let Ok(parsed) = frontmatter::parse_or_empty(&import_content)
    {
        let mut fm = parsed.frontmatter;

        if !fm.contains_key("part_of") {
            // Read the parent's title from its frontmatter if available.
            let parent_title = host::fs::read_file(&graft_target)
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
                Value::String(format_link_with_format(
                    &graft_target,
                    &parent_title,
                    link_format,
                    &import_index_path,
                )),
            );

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                let _ = host::fs::write_file(&import_index_path, &updated);
            }
        }
    }
}

/// Find the workspace root index file (has `contents` but no `part_of`).
fn find_root_index() -> Option<String> {
    let files = host::fs::list_files("").ok()?;
    // Look for root-level markdown files (no slashes in path, or just "index.md")
    for file in &files {
        // Only check root-level files
        if file.contains('/') {
            continue;
        }
        if !file.ends_with(".md") {
            continue;
        }
        if let Ok(content) = host::fs::read_file(file) {
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

fn workspace_link_format(parent_path: Option<&str>) -> LinkFormat {
    let parent_path = normalize_optional_path(parent_path);
    find_root_index()
        .as_deref()
        .and_then(read_link_format_from_file)
        .or_else(|| parent_path.and_then(detect_link_format_from_file))
        .unwrap_or_default()
}

fn read_link_format_from_file(path: &str) -> Option<LinkFormat> {
    let content = host::fs::read_file(path).ok()?;
    let parsed = frontmatter::parse_or_empty(&content).ok()?;
    parsed
        .frontmatter
        .get("link_format")
        .and_then(|v| v.as_str())
        .and_then(parse_link_format)
}

fn detect_link_format_from_file(path: &str) -> Option<LinkFormat> {
    let content = host::fs::read_file(path).ok()?;
    let parsed = frontmatter::parse_or_empty(&content).ok()?;

    for key in ["part_of", "attachments"] {
        if let Some(value) = parsed.frontmatter.get(key).and_then(|v| v.as_str()) {
            return Some(detect_link_format(value));
        }
    }

    parsed
        .frontmatter
        .get("contents")
        .and_then(|v| v.as_sequence())
        .and_then(|seq| seq.first())
        .and_then(|v| v.as_str())
        .map(detect_link_format)
}

fn parse_link_format(raw: &str) -> Option<LinkFormat> {
    match raw {
        "markdown_root" => Some(LinkFormat::MarkdownRoot),
        "markdown_relative" => Some(LinkFormat::MarkdownRelative),
        "plain_relative" => Some(LinkFormat::PlainRelative),
        "plain_canonical" => Some(LinkFormat::PlainCanonical),
        _ => None,
    }
}

fn detect_link_format(link: &str) -> LinkFormat {
    let parsed = parse_link(link);
    let is_markdown = parsed.title.is_some();

    match (is_markdown, parsed.path_type) {
        (true, PathType::WorkspaceRoot) => LinkFormat::MarkdownRoot,
        (true, PathType::Relative | PathType::Ambiguous) => LinkFormat::MarkdownRelative,
        (false, PathType::WorkspaceRoot) => LinkFormat::PlainCanonical,
        (false, PathType::Relative | PathType::Ambiguous) => LinkFormat::PlainRelative,
    }
}

// ── Helper functions ──────────────────────────────────────────────────

/// Format an ImportedEntry as a markdown string with frontmatter links.
fn format_entry(
    entry: &ImportedEntry,
    entry_path: &str,
    month_index_canonical: &str,
    link_format: LinkFormat,
) -> String {
    let mut fm = IndexMap::new();

    fm.insert("title".to_string(), Value::String(entry.title.clone()));

    // link: self-link.
    fm.insert(
        "link".to_string(),
        Value::String(format_link_with_format(
            entry_path,
            &entry.title,
            link_format,
            entry_path,
        )),
    );

    // Add extra metadata (from, to, cc, tags, etc.).
    for (key, value) in &entry.metadata {
        fm.insert(key.clone(), value.clone());
    }

    if let Some(dt) = entry.date {
        fm.insert("date".to_string(), Value::String(dt.to_rfc3339()));
    }

    // part_of: link to month index.
    let (year, month, _) = date_components(entry);
    let month_title = format!("{year}-{month}");
    fm.insert(
        "part_of".to_string(),
        Value::String(format_link_with_format(
            month_index_canonical,
            &month_title,
            link_format,
            entry_path,
        )),
    );

    // Attachments list.
    if !entry.attachments.is_empty() {
        let entry_stem = path_stem(entry_path);
        let entry_dir = path_parent(entry_path);
        let att_list: Vec<Value> = entry
            .attachments
            .iter()
            .map(|a| {
                let note_path = if entry_dir.is_empty() {
                    format!("{entry_stem}/_attachments/{}.md", a.filename)
                } else {
                    format!("{entry_dir}/{entry_stem}/_attachments/{}.md", a.filename)
                };
                Value::String(format_link_with_format(
                    &note_path,
                    &a.filename,
                    link_format,
                    entry_path,
                ))
            })
            .collect();
        fm.insert("attachments".to_string(), Value::Sequence(att_list));
    }

    let yaml = serde_yaml_ng::to_string(&fm).unwrap_or_default();

    // Resolve _attachments/ references in the body to include the entry stem.
    let body = if !entry.attachments.is_empty() {
        let entry_stem = path_stem(entry_path);
        let entry_dir = path_parent(entry_path);
        let mut body = entry.body.clone();
        for attachment in &entry.attachments {
            let canonical_path = if entry_dir.is_empty() {
                format!("{entry_stem}/_attachments/{}", attachment.filename)
            } else {
                format!(
                    "{entry_dir}/{entry_stem}/_attachments/{}",
                    attachment.filename
                )
            };
            body = body.replace(
                &format!("_attachments/{}", attachment.filename),
                &format_body_attachment_path(&canonical_path, entry_path, link_format),
            );
        }
        body
    } else {
        entry.body.clone()
    };

    format!("---\n{yaml}---\n{body}")
}

fn format_body_attachment_path(
    canonical_path: &str,
    from_canonical_path: &str,
    link_format: LinkFormat,
) -> String {
    let path = match link_format {
        LinkFormat::MarkdownRoot => format!("/{canonical_path}"),
        LinkFormat::MarkdownRelative | LinkFormat::PlainRelative => {
            compute_relative_path(from_canonical_path, canonical_path)
        }
        LinkFormat::PlainCanonical => canonical_path.to_string(),
    };
    format_markdown_url(&path)
}

/// Create a markdown file serving as an attachment note for a binary file.
fn format_attachment_note(
    filename: &str,
    binary_path: &str,
    note_path: &str,
    entry_path: &str,
    entry_title: &str,
    link_format: LinkFormat,
) -> String {
    let mut fm = IndexMap::new();
    fm.insert("title".to_string(), Value::String(filename.to_string()));
    fm.insert(
        "link".to_string(),
        Value::String(format_link_with_format(
            note_path,
            filename,
            link_format,
            note_path,
        )),
    );
    fm.insert(
        "attachment".to_string(),
        Value::String(format_link_with_format(
            binary_path,
            filename,
            link_format,
            note_path,
        )),
    );
    fm.insert(
        "attachment_of".to_string(),
        Value::Sequence(vec![Value::String(format_link_with_format(
            entry_path,
            entry_title,
            link_format,
            note_path,
        ))]),
    );

    let yaml = serde_yaml_ng::to_string(&fm).unwrap_or_default();
    format!("---\n{yaml}---\n")
}

fn format_markdown_url(path: &str) -> String {
    if path.contains(' ') || path.contains('(') || path.contains(')') {
        format!("<{path}>")
    } else {
        path.to_string()
    }
}

fn normalize_optional_path(path: Option<&str>) -> Option<&str> {
    path.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Extract (year, month, date_prefix) from an entry's date or fall back to today.
fn date_components(entry: &ImportedEntry) -> (String, String, String) {
    if let Some(dt) = entry.date {
        let year = dt.format("%Y").to_string();
        let month = dt.format("%m").to_string();
        let date_prefix = dt.format("%Y-%m-%d").to_string();
        return (year, month, date_prefix);
    }

    // Keep imports deterministic when source entries do not include a date.
    (
        "1970".to_string(),
        "01".to_string(),
        "1970-01-01".to_string(),
    )
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
