//! CLI handler for `diaryx import` subcommands.
//!
//! Responsible for filesystem I/O — the core import module stays pure.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use diaryx_core::entry::slugify;
use diaryx_core::import::dayone;
use diaryx_core::import::email;
use diaryx_core::import::markdown;
use diaryx_core::import::{ImportResult, ImportedEntry};
use diaryx_core::link_parser::format_link;
use indexmap::IndexMap;
use serde_yaml::Value;

use super::args::ImportCommands;

/// Dispatch import sub-commands.
pub fn handle_import_command(command: ImportCommands, workspace_arg: Option<PathBuf>) {
    match command {
        ImportCommands::Email {
            source,
            folder,
            dry_run,
            verbose,
        } => handle_import_email(&source, &folder, dry_run, verbose, workspace_arg),
        ImportCommands::DayOne {
            source,
            folder,
            dry_run,
            verbose,
        } => handle_import_dayone(&source, &folder, dry_run, verbose, workspace_arg),
        ImportCommands::Markdown {
            source,
            folder,
            dry_run,
            verbose,
        } => {
            let folder_name = folder.unwrap_or_else(|| {
                source
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("imported")
                    .to_string()
            });
            handle_import_markdown(&source, &folder_name, dry_run, verbose, workspace_arg);
        }
    }
}

/// Import emails from a file, directory, or mbox archive.
fn handle_import_email(
    source: &Path,
    folder: &str,
    dry_run: bool,
    verbose: bool,
    workspace_arg: Option<PathBuf>,
) {
    let workspace_root = super::resolve_workspace_root(workspace_arg);

    if !source.exists() {
        eprintln!("Error: source path does not exist: {}", source.display());
        return;
    }

    // Collect parsed entries
    let entries = if source.is_dir() {
        parse_directory(source, verbose)
    } else {
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "mbox" | "mbx" => {
                let results = email::parse_mbox(source);
                collect_results(results, verbose)
            }
            _ => {
                // Assume single .eml
                let bytes = match std::fs::read(source) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("Error reading {}: {e}", source.display());
                        return;
                    }
                };
                match email::parse_eml(&bytes) {
                    Ok(entry) => vec![entry],
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return;
                    }
                }
            }
        }
    };

    if entries.is_empty() {
        println!("No emails to import.");
        return;
    }

    let result = write_entries(&workspace_root, folder, &entries, dry_run, verbose);

    // Print summary
    println!();
    if dry_run {
        println!("Dry run — no files were written.");
    }
    println!(
        "Imported: {}  Skipped: {}  Attachments: {}",
        result.imported, result.skipped, result.attachment_count
    );
    for err in &result.errors {
        eprintln!("  Warning: {err}");
    }
}

/// Import entries from a Day One Journal.json export.
fn handle_import_dayone(
    source: &Path,
    folder: &str,
    dry_run: bool,
    verbose: bool,
    workspace_arg: Option<PathBuf>,
) {
    let workspace_root = super::resolve_workspace_root(workspace_arg);

    if !source.exists() {
        eprintln!("Error: source path does not exist: {}", source.display());
        return;
    }

    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading {}: {e}", source.display());
            return;
        }
    };

    let entries = collect_results(dayone::parse_dayone(&bytes), verbose);

    if entries.is_empty() {
        println!("No entries to import.");
        return;
    }

    let result = write_entries(&workspace_root, folder, &entries, dry_run, verbose);

    // Print summary
    println!();
    if dry_run {
        println!("Dry run — no files were written.");
    }
    println!(
        "Imported: {}  Skipped: {}  Attachments: {}",
        result.imported, result.skipped, result.attachment_count
    );
    for err in &result.errors {
        eprintln!("  Warning: {err}");
    }
}

/// Parse all .eml files in a directory (non-recursive).
fn parse_directory(dir: &Path, verbose: bool) -> Vec<ImportedEntry> {
    let mut entries = Vec::new();

    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("eml"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => {
            eprintln!("Error reading directory {}: {e}", dir.display());
            return entries;
        }
    };
    paths.sort();

    for path in &paths {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                if verbose {
                    eprintln!("  Skipping {}: {e}", path.display());
                }
                continue;
            }
        };
        match email::parse_eml(&bytes) {
            Ok(entry) => {
                if verbose {
                    println!("  Parsed: {}", entry.title);
                }
                entries.push(entry);
            }
            Err(e) => {
                if verbose {
                    eprintln!("  Skipping {}: {e}", path.display());
                }
            }
        }
    }

    entries
}

/// Collect parsed results from mbox, filtering errors.
fn collect_results(
    results: Vec<Result<ImportedEntry, String>>,
    verbose: bool,
) -> Vec<ImportedEntry> {
    let mut entries = Vec::new();
    for result in results {
        match result {
            Ok(entry) => {
                if verbose {
                    println!("  Parsed: {}", entry.title);
                }
                entries.push(entry);
            }
            Err(e) => {
                if verbose {
                    eprintln!("  Skipping: {e}");
                }
            }
        }
    }
    entries
}

/// Compute the canonical path (workspace-relative, forward slashes) from an absolute path.
fn canonical_path(workspace_root: &Path, abs_path: &Path) -> String {
    abs_path
        .strip_prefix(workspace_root)
        .unwrap_or(abs_path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Write imported entries to the workspace, creating the folder hierarchy.
fn write_entries(
    workspace_root: &Path,
    folder: &str,
    entries: &[ImportedEntry],
    dry_run: bool,
    verbose: bool,
) -> ImportResult {
    let base_dir = workspace_root.join(folder);
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
        attachment_count: 0,
    };

    // Track used filenames within each directory to handle collisions
    let mut used_paths: HashSet<PathBuf> = HashSet::new();

    // Track hierarchy: canonical paths for index files, plus their child links
    // year_key (canonical) -> set of month canonical paths
    let mut year_to_months: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
    // month_key (canonical) -> ordered list of entry links
    let mut month_to_entries: IndexMap<String, Vec<String>> = IndexMap::new();
    // All year canonical paths (for root index)
    let mut all_years: IndexMap<String, String> = IndexMap::new(); // canonical -> title

    for entry in entries {
        let (year, month, date_prefix) = date_components(entry);
        let slug = entry_slug(&entry.title);
        let filename = format!("{date_prefix}-{slug}.md");

        let month_dir = base_dir.join(&year).join(&month);
        let mut entry_path = month_dir.join(&filename);

        // Handle filename collisions
        entry_path = deduplicate_path(entry_path, &mut used_paths);
        used_paths.insert(entry_path.clone());

        // Compute canonical paths for hierarchy tracking
        let entry_canonical = canonical_path(workspace_root, &entry_path);
        let month_index_canonical = format!("{folder}/{year}/{month}/{year}_{month}.md");
        let year_index_canonical = format!("{folder}/{year}/{year}_index.md");

        // Track: root -> years
        all_years
            .entry(year_index_canonical.clone())
            .or_insert_with(|| year.clone());

        // Track: year -> months
        year_to_months
            .entry(year_index_canonical)
            .or_default()
            .entry(month_index_canonical.clone())
            .or_insert_with(|| format!("{year}-{month}"));

        // Track: month -> entries (as MarkdownRoot links)
        let entry_link = format_link(&entry_canonical, &entry.title);
        month_to_entries
            .entry(month_index_canonical.clone())
            .or_default()
            .push(entry_link);

        // Build entry markdown with MarkdownRoot links
        let entry_content =
            format_entry(entry, &entry_path, workspace_root, &month_index_canonical);

        if verbose {
            let rel = entry_path
                .strip_prefix(workspace_root)
                .unwrap_or(&entry_path);
            if dry_run {
                println!("  Would write: {}", rel.display());
            } else {
                println!("  Writing: {}", rel.display());
            }
        }

        if !dry_run {
            if let Err(e) = std::fs::create_dir_all(&month_dir) {
                result.errors.push(format!(
                    "Failed to create directory {}: {e}",
                    month_dir.display()
                ));
                result.skipped += 1;
                continue;
            }
            if let Err(e) = std::fs::write(&entry_path, &entry_content) {
                result
                    .errors
                    .push(format!("Failed to write {}: {e}", entry_path.display()));
                result.skipped += 1;
                continue;
            }
        }

        // Write attachments
        if !entry.attachments.is_empty() {
            let entry_stem = entry_path.file_stem().unwrap().to_string_lossy();
            let attachments_dir = month_dir.join(format!("{entry_stem}/_attachments"));

            for att in &entry.attachments {
                let att_path = attachments_dir.join(&att.filename);
                if verbose {
                    let rel = att_path.strip_prefix(workspace_root).unwrap_or(&att_path);
                    if dry_run {
                        println!("    Would write attachment: {}", rel.display());
                    } else {
                        println!("    Writing attachment: {}", rel.display());
                    }
                }
                if !dry_run {
                    if let Err(e) = std::fs::create_dir_all(&attachments_dir) {
                        result
                            .errors
                            .push(format!("Failed to create attachment dir: {e}"));
                        continue;
                    }
                    if let Err(e) = std::fs::write(&att_path, &att.data) {
                        result
                            .errors
                            .push(format!("Failed to write attachment: {e}"));
                        continue;
                    }
                }
                result.attachment_count += 1;
            }
        }

        result.imported += 1;
    }

    // Write index hierarchy
    if !dry_run {
        write_index_hierarchy(
            workspace_root,
            &base_dir,
            folder,
            &all_years,
            &year_to_months,
            &month_to_entries,
            verbose,
        );
    }

    result
}

/// Format an ImportedEntry as a markdown string with MarkdownRoot-style frontmatter links.
fn format_entry(
    entry: &ImportedEntry,
    entry_path: &Path,
    workspace_root: &Path,
    month_index_canonical: &str,
) -> String {
    let mut fm = IndexMap::new();

    fm.insert("title".to_string(), Value::String(entry.title.clone()));

    // Add email metadata (from, to, cc)
    for (key, value) in &entry.metadata {
        fm.insert(key.clone(), value.clone());
    }

    if let Some(dt) = entry.date {
        fm.insert("date".to_string(), Value::String(dt.to_rfc3339()));
    }

    // part_of: MarkdownRoot link to month index
    let (year, month, _) = date_components_from_datetime(entry.date);
    let month_title = format!("{year}-{month}");
    fm.insert(
        "part_of".to_string(),
        Value::String(format_link(month_index_canonical, &month_title)),
    );

    // Attachments list
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
    format!("---\n{yaml}---\n{}", entry.body)
}

/// Write the index.md, year index, and month index files with MarkdownRoot links.
fn write_index_hierarchy(
    workspace_root: &Path,
    base_dir: &Path,
    folder: &str,
    all_years: &IndexMap<String, String>,
    year_to_months: &IndexMap<String, IndexMap<String, String>>,
    month_to_entries: &IndexMap<String, Vec<String>>,
    verbose: bool,
) {
    let root_index_canonical = format!("{folder}/index.md");

    // Root index: emails/index.md
    let root_index = base_dir.join("index.md");
    if !root_index.exists() {
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

        if verbose {
            println!("  Writing index: {}", root_index.display());
        }
        let _ = std::fs::create_dir_all(base_dir);
        let _ = std::fs::write(&root_index, content);
    }

    // Year indexes
    for (year_canonical, months) in year_to_months {
        let year_path = workspace_root.join(year_canonical);
        if !year_path.exists() {
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

            if verbose {
                println!("  Writing index: {}", year_path.display());
            }
            let year_dir = year_path.parent().unwrap();
            let _ = std::fs::create_dir_all(year_dir);
            let _ = std::fs::write(&year_path, content);
        }
    }

    // Month indexes
    for (month_canonical, entry_links) in month_to_entries {
        let month_path = workspace_root.join(month_canonical);
        if !month_path.exists() {
            let month_title = month_path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .replace('_', "-");

            // Find parent year canonical
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

            if verbose {
                println!("  Writing index: {}", month_path.display());
            }
            let month_dir = month_path.parent().unwrap();
            let _ = std::fs::create_dir_all(month_dir);
            let _ = std::fs::write(&month_path, content);
        }
    }
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

/// Create a URL-safe slug from a title, or fall back to "untitled-email".
fn entry_slug(title: &str) -> String {
    let slug = slugify(title);
    if slug.is_empty() {
        "untitled-email".to_string()
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

// ── Markdown directory import ──────────────────────────────────────────

/// Directories to skip when walking a markdown source tree.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".svn",
    "dist",
    "build",
    "__pycache__",
    ".next",
    ".nuxt",
    "vendor",
    ".cargo",
    ".obsidian",
    ".trash",
    ".diaryx",
];

/// Import a directory of markdown files, preserving directory structure.
fn handle_import_markdown(
    source: &Path,
    folder: &str,
    dry_run: bool,
    verbose: bool,
    workspace_arg: Option<PathBuf>,
) {
    let workspace_root = super::resolve_workspace_root(workspace_arg);

    if !source.exists() {
        eprintln!("Error: source path does not exist: {}", source.display());
        return;
    }
    if !source.is_dir() {
        eprintln!("Error: source must be a directory: {}", source.display());
        return;
    }

    let base_dir = workspace_root.join(folder);
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
        attachment_count: 0,
    };

    // Step 1: Walk source directory, collecting files and directory structure.
    let mut md_files: Vec<(PathBuf, String)> = Vec::new(); // (source_path, relative_path)
    let mut non_md_files: Vec<(PathBuf, String)> = Vec::new(); // (source_path, relative_path)
    let mut directories: HashSet<String> = HashSet::new(); // relative dir paths

    walk_source_dir(
        source,
        source,
        &mut md_files,
        &mut non_md_files,
        &mut directories,
    );

    if md_files.is_empty() && non_md_files.is_empty() {
        println!("No files found in source directory.");
        return;
    }

    // Step 2: Build complete index mapping.
    // First, detect existing indexes in the source (by name or `contents` frontmatter).
    let mut dir_has_index: IndexMap<String, String> = IndexMap::new(); // dir_rel -> relative path of the index file

    for (src_path, rel_path) in &md_files {
        let file_name = src_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let dir_rel = parent_rel_path(rel_path);

        let is_index_by_name = file_name == "index.md" || file_name.ends_with("_index.md");

        let is_index_by_contents = if !is_index_by_name {
            std::fs::read(src_path)
                .ok()
                .and_then(|bytes| {
                    let content = std::str::from_utf8(&bytes).ok()?;
                    let parsed = diaryx_core::frontmatter::parse_or_empty(content).ok()?;
                    if parsed.frontmatter.contains_key("contents") {
                        Some(true)
                    } else {
                        None
                    }
                })
                .unwrap_or(false)
        } else {
            false
        };

        if is_index_by_name || is_index_by_contents {
            dir_has_index.insert(dir_rel, rel_path.clone());
        }
    }

    // Pre-register generated indexes for directories that don't already have one.
    // This lets step 4 compute correct part_of links before the indexes are written.
    let mut all_dirs: Vec<String> = directories.iter().cloned().collect();
    if !all_dirs.contains(&String::new()) {
        all_dirs.push(String::new());
    }
    // Sort deepest-first so child indexes are registered before parents need them.
    all_dirs.sort_by(|a, b| {
        let depth_a = if a.is_empty() {
            0
        } else {
            a.matches('/').count() + 1
        };
        let depth_b = if b.is_empty() {
            0
        } else {
            b.matches('/').count() + 1
        };
        depth_b.cmp(&depth_a)
    });

    for dir_rel in &all_dirs {
        if dir_has_index.contains_key(dir_rel.as_str()) {
            continue;
        }
        let dir_name = if dir_rel.is_empty() {
            folder.to_string()
        } else {
            Path::new(dir_rel)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index")
                .to_string()
        };
        let index_filename = if dir_rel.is_empty() {
            "index.md".to_string()
        } else {
            format!("{dir_name}_index.md")
        };
        let index_rel = if dir_rel.is_empty() {
            index_filename
        } else {
            format!("{dir_rel}/{index_filename}")
        };
        dir_has_index.insert(dir_rel.clone(), index_rel);
    }

    // Step 3: Copy non-markdown files.
    // Track which non-md files are in which directory (for attachments frontmatter).
    let mut dir_attachments: IndexMap<String, Vec<String>> = IndexMap::new(); // dir_rel -> list of filenames

    for (src_path, rel_path) in &non_md_files {
        let dest_path = base_dir.join(rel_path);
        let dir_rel = parent_rel_path(rel_path);
        let filename = src_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        dir_attachments.entry(dir_rel).or_default().push(filename);

        if verbose {
            let display_rel = Path::new(folder).join(rel_path);
            if dry_run {
                println!("  Would copy: {}", display_rel.display());
            } else {
                println!("  Copying: {}", display_rel.display());
            }
        }

        if !dry_run {
            if let Some(parent) = dest_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    result.errors.push(format!(
                        "Failed to create directory {}: {e}",
                        parent.display()
                    ));
                    continue;
                }
            }
            if let Err(e) = std::fs::copy(src_path, &dest_path) {
                result
                    .errors
                    .push(format!("Failed to copy {}: {e}", src_path.display()));
                continue;
            }
        }
        result.attachment_count += 1;
    }

    // Step 4: Process markdown files — parse and write with augmented frontmatter.
    for (src_path, rel_path) in &md_files {
        let bytes = match std::fs::read(src_path) {
            Ok(b) => b,
            Err(e) => {
                if verbose {
                    eprintln!("  Skipping {}: {e}", src_path.display());
                }
                result.skipped += 1;
                result
                    .errors
                    .push(format!("Failed to read {}: {e}", src_path.display()));
                continue;
            }
        };

        let filename = src_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.md");

        let entry = match markdown::parse_markdown_file(&bytes, filename) {
            Ok(e) => e,
            Err(e) => {
                if verbose {
                    eprintln!("  Skipping {}: {e}", src_path.display());
                }
                result.skipped += 1;
                result.errors.push(e);
                continue;
            }
        };

        let dir_rel = parent_rel_path(rel_path);
        let dest_path = base_dir.join(rel_path);

        // Determine the index file this entry should be part_of
        let index_canonical = dir_index_canonical(folder, &dir_rel, &dir_has_index);

        // Build augmented frontmatter
        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String(entry.title.clone()));

        // Preserve original metadata
        for (key, value) in &entry.metadata {
            fm.insert(key.clone(), value.clone());
        }

        if let Some(dt) = entry.date {
            fm.insert("date".to_string(), Value::String(dt.to_rfc3339()));
        }

        // Add part_of link (unless this IS the root index)
        if let Some(ref idx_canon) = index_canonical {
            let idx_title = index_title_from_canonical(idx_canon);
            fm.insert(
                "part_of".to_string(),
                Value::String(format_link(idx_canon, &idx_title)),
            );
        }

        // Check if this file is an index (has contents in dir_has_index)
        let is_this_an_index = dir_has_index.values().any(|v| v == rel_path);
        if is_this_an_index {
            // We'll fill in contents later after all files are known.
            // For now, keep any contents from the original if the user had them — but we stripped them.
            // The index hierarchy step below will handle adding contents.
        }

        let content = diaryx_core::frontmatter::serialize(&fm, &entry.body)
            .unwrap_or_else(|_| format!("---\ntitle: {}\n---\n{}", entry.title, entry.body));

        if verbose {
            let display_rel = Path::new(folder).join(rel_path);
            if dry_run {
                println!("  Would write: {}", display_rel.display());
            } else {
                println!("  Writing: {}", display_rel.display());
            }
        }

        if !dry_run {
            if let Some(parent) = dest_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    result.errors.push(format!(
                        "Failed to create directory {}: {e}",
                        parent.display()
                    ));
                    result.skipped += 1;
                    continue;
                }
            }
            if let Err(e) = std::fs::write(&dest_path, &content) {
                result
                    .errors
                    .push(format!("Failed to write {}: {e}", dest_path.display()));
                result.skipped += 1;
                continue;
            }
        }

        result.imported += 1;
    }

    // Step 5: Create index files for directories that don't have a source index.
    // all_dirs is already sorted deepest-first from step 2.
    for dir_rel in &all_dirs {
        // Skip if this directory had an index from the source (already written in step 4)
        if md_files.iter().any(|(_, r)| {
            dir_has_index
                .get(dir_rel.as_str())
                .map(|v| v == r)
                .unwrap_or(false)
        }) {
            continue;
        }

        let index_rel = match dir_has_index.get(dir_rel.as_str()) {
            Some(r) => r.clone(),
            None => continue, // Should not happen — all dirs were pre-registered
        };

        let index_dest = base_dir.join(&index_rel);

        // Collect children: md files and sub-directory indexes in this directory
        let mut contents_links: Vec<Value> = Vec::new();

        // Add child markdown files (excluding indexes)
        for (_src_path, rel_path) in &md_files {
            let file_dir = parent_rel_path(rel_path);
            if file_dir != *dir_rel {
                continue;
            }
            // Skip if this file is an index for its directory
            if dir_has_index.values().any(|v| v == rel_path) {
                continue;
            }
            let child_canonical = format!("{folder}/{rel_path}");
            let child_title = child_title_from_path(rel_path);
            contents_links.push(Value::String(format_link(&child_canonical, &child_title)));
        }

        // Add child directory indexes
        for child_dir in &all_dirs {
            if child_dir.is_empty() && !dir_rel.is_empty() {
                continue;
            }
            if child_dir == dir_rel {
                continue;
            }
            let child_parent = parent_rel_path(child_dir);
            if child_parent != *dir_rel {
                continue;
            }
            if let Some(child_index_rel) = dir_has_index.get(child_dir.as_str()) {
                let child_canonical = format!("{folder}/{child_index_rel}");
                let child_title = child_title_from_path(child_index_rel);
                contents_links.push(Value::String(format_link(&child_canonical, &child_title)));
            }
        }

        // Build index frontmatter
        let dir_name = if dir_rel.is_empty() {
            folder.to_string()
        } else {
            Path::new(dir_rel)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index")
                .to_string()
        };
        let title = diaryx_core::entry::prettify_filename(&dir_name);
        let mut fm = IndexMap::new();
        fm.insert("title".to_string(), Value::String(title.clone()));

        // part_of: link to parent directory's index (unless this is the root)
        if !dir_rel.is_empty() {
            let parent_dir = parent_rel_path(dir_rel);
            if let Some(parent_index_canon) =
                dir_index_canonical(folder, &parent_dir, &dir_has_index)
            {
                let parent_title = index_title_from_canonical(&parent_index_canon);
                fm.insert(
                    "part_of".to_string(),
                    Value::String(format_link(&parent_index_canon, &parent_title)),
                );
            }
        }

        // contents
        if !contents_links.is_empty() {
            fm.insert("contents".to_string(), Value::Sequence(contents_links));
        }

        // attachments: non-md files in this directory
        if let Some(atts) = dir_attachments.get(dir_rel.as_str()) {
            if !atts.is_empty() {
                let att_values: Vec<Value> = atts
                    .iter()
                    .map(|filename| {
                        let att_rel = if dir_rel.is_empty() {
                            filename.clone()
                        } else {
                            format!("{dir_rel}/{filename}")
                        };
                        Value::String(format!("{folder}/{att_rel}"))
                    })
                    .collect();
                fm.insert("attachments".to_string(), Value::Sequence(att_values));
            }
        }

        let content = diaryx_core::frontmatter::serialize(&fm, "")
            .unwrap_or_else(|_| format!("---\ntitle: {title}\n---\n"));

        if verbose {
            let display_rel = Path::new(folder).join(&index_rel);
            if dry_run {
                println!("  Would create index: {}", display_rel.display());
            } else {
                println!("  Creating index: {}", display_rel.display());
            }
        }

        if !dry_run {
            if let Some(parent) = index_dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&index_dest, &content) {
                result.errors.push(format!(
                    "Failed to write index {}: {e}",
                    index_dest.display()
                ));
            }
        }
    }

    // Step 6: Update existing source indexes with part_of links.
    // These were already written in step 4, but without part_of (since the parent
    // indexes might not have existed yet). Re-write them with part_of added.
    if !dry_run {
        for (dir_rel, index_rel) in &dir_has_index {
            // Skip generated indexes (they already have correct part_of)
            if !md_files.iter().any(|(_, r)| r == index_rel) {
                continue;
            }

            let dest_path = base_dir.join(index_rel);
            if !dest_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&dest_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let parsed = match diaryx_core::frontmatter::parse_or_empty(&content) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let mut fm = parsed.frontmatter;

            // Add part_of if missing
            if !fm.contains_key("part_of") {
                let parent_dir = parent_rel_path(dir_rel);
                if let Some(parent_canon) = dir_index_canonical(folder, &parent_dir, &dir_has_index)
                {
                    let parent_title = index_title_from_canonical(&parent_canon);
                    fm.insert(
                        "part_of".to_string(),
                        Value::String(format_link(&parent_canon, &parent_title)),
                    );
                }
            }

            // Add contents listing children
            if !fm.contains_key("contents") {
                let mut contents_links: Vec<Value> = Vec::new();

                // Child md files (not indexes)
                for (_, rel_path) in &md_files {
                    let file_dir = parent_rel_path(rel_path);
                    if file_dir != *dir_rel {
                        continue;
                    }
                    if dir_has_index.values().any(|v| v == rel_path) {
                        continue;
                    }
                    let child_canonical = format!("{folder}/{rel_path}");
                    let child_title = child_title_from_path(rel_path);
                    contents_links.push(Value::String(format_link(&child_canonical, &child_title)));
                }

                // Child directory indexes
                for child_dir in &all_dirs {
                    if child_dir == dir_rel || child_dir.is_empty() {
                        continue;
                    }
                    let child_parent = parent_rel_path(child_dir);
                    if child_parent != *dir_rel {
                        continue;
                    }
                    if let Some(child_index_rel) = dir_has_index.get(child_dir.as_str()) {
                        let child_canonical = format!("{folder}/{child_index_rel}");
                        let child_title = child_title_from_path(child_index_rel);
                        contents_links
                            .push(Value::String(format_link(&child_canonical, &child_title)));
                    }
                }

                if !contents_links.is_empty() {
                    fm.insert("contents".to_string(), Value::Sequence(contents_links));
                }
            }

            // Add attachments for this directory
            if !fm.contains_key("attachments") {
                if let Some(atts) = dir_attachments.get(dir_rel.as_str()) {
                    if !atts.is_empty() {
                        let att_values: Vec<Value> = atts
                            .iter()
                            .map(|filename| {
                                let att_rel = if dir_rel.is_empty() {
                                    filename.clone()
                                } else {
                                    format!("{dir_rel}/{filename}")
                                };
                                Value::String(format!("{folder}/{att_rel}"))
                            })
                            .collect();
                        fm.insert("attachments".to_string(), Value::Sequence(att_values));
                    }
                }
            }

            if let Ok(updated) = diaryx_core::frontmatter::serialize(&fm, &parsed.body) {
                let _ = std::fs::write(&dest_path, updated);
            }
        }
    }

    // Print summary
    println!();
    if dry_run {
        println!("Dry run — no files were written.");
    }
    println!(
        "Imported: {}  Skipped: {}  Attachments: {}",
        result.imported, result.skipped, result.attachment_count
    );
    for err in &result.errors {
        eprintln!("  Warning: {err}");
    }
}

/// Recursively walk a source directory, collecting markdown and non-markdown files.
fn walk_source_dir(
    root: &Path,
    current: &Path,
    md_files: &mut Vec<(PathBuf, String)>,
    non_md_files: &mut Vec<(PathBuf, String)>,
    directories: &mut HashSet<String>,
) {
    let mut entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(current) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden files/dirs
        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Skip build/dependency directories
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }

            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            directories.insert(rel);

            walk_source_dir(root, &path, md_files, non_md_files, directories);
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let is_md = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false);

            if is_md {
                md_files.push((path.clone(), rel));
            } else {
                non_md_files.push((path.clone(), rel));
            }
        }
    }
}

/// Get the parent relative path from a relative file path.
/// Returns "" for files at the root level.
fn parent_rel_path(rel_path: &str) -> String {
    match rel_path.rfind('/') {
        Some(idx) => rel_path[..idx].to_string(),
        None => String::new(),
    }
}

/// Get the canonical path for a directory's index file.
fn dir_index_canonical(
    folder: &str,
    dir_rel: &str,
    dir_has_index: &IndexMap<String, String>,
) -> Option<String> {
    dir_has_index
        .get(dir_rel)
        .map(|index_rel| format!("{folder}/{index_rel}"))
}

/// Extract a title from an index canonical path.
fn index_title_from_canonical(canonical: &str) -> String {
    let filename = Path::new(canonical)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Index");
    let cleaned = filename.strip_suffix("_index").unwrap_or(filename);
    diaryx_core::entry::prettify_filename(cleaned)
}

/// Get a title for a child file from its relative path.
fn child_title_from_path(rel_path: &str) -> String {
    let filename = Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let cleaned = filename.strip_suffix("_index").unwrap_or(filename);
    diaryx_core::entry::prettify_filename(cleaned)
}
