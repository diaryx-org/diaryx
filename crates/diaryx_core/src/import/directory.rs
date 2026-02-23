//! In-place directory import: convert a directory of markdown files to Diaryx
//! hierarchy format by adding `part_of`/`contents`/`attachments` frontmatter.
//!
//! This is the async, `AsyncFileSystem`-based equivalent of the CLI's
//! `handle_import_markdown`. Unlike the CLI version, this operates **in-place**:
//! files are not copied, only augmented with hierarchy metadata.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::entry::prettify_filename;
use crate::error::Result;
use crate::frontmatter;
use crate::fs::AsyncFileSystem;
use crate::metadata_writer;

use super::ImportResult;

/// Directories to skip when walking a source tree.
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

/// Convert a directory of markdown files to Diaryx hierarchy format in-place.
///
/// Walks the directory tree rooted at `root`, detects or creates index files,
/// and adds `part_of`/`contents`/`attachments` frontmatter to build the
/// Diaryx workspace hierarchy.
///
/// This operation is idempotent: files that already have correct metadata are
/// skipped. Running it twice produces the same result.
pub async fn import_directory_in_place<FS: AsyncFileSystem>(
    fs: &FS,
    root: &Path,
) -> Result<ImportResult> {
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
        attachment_count: 0,
    };

    // --- Phase 1: Walk & classify ---
    let mut md_files: Vec<String> = Vec::new(); // relative paths
    let mut non_md_files: Vec<String> = Vec::new();
    let mut directories: HashSet<String> = HashSet::new();
    directories.insert(String::new()); // root directory

    walk_dir(
        fs,
        root,
        root,
        &mut md_files,
        &mut non_md_files,
        &mut directories,
    )
    .await;

    if md_files.is_empty() && non_md_files.is_empty() {
        return Ok(result);
    }

    // --- Phase 2: Detect existing indexes ---
    // Map from directory relative path → index file relative path
    let mut dir_index_map: IndexMap<String, String> = IndexMap::new();

    for rel_path in &md_files {
        let filename = file_name(rel_path);
        let dir_rel = parent_rel_path(rel_path);

        let is_index_by_name = filename == "index.md" || filename.ends_with("_index.md");

        let is_index_by_contents = if !is_index_by_name {
            let full_path = join_path(root, rel_path);
            match fs.read_to_string(&full_path).await {
                Ok(content) => match frontmatter::parse_or_empty(&content) {
                    Ok(parsed) => parsed.frontmatter.contains_key("contents"),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        } else {
            false
        };

        if is_index_by_name || is_index_by_contents {
            dir_index_map.insert(dir_rel, rel_path.clone());
        }
    }

    // --- Phase 3: Register missing indexes ---
    let mut all_dirs: Vec<String> = directories.iter().cloned().collect();
    // Sort deepest-first so child indexes are registered before parents reference them.
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
        if dir_index_map.contains_key(dir_rel.as_str()) {
            continue;
        }
        let dir_name = if dir_rel.is_empty() {
            // Use the root directory name, or "index" as fallback
            root.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index")
                .to_string()
        } else {
            dir_rel.rsplit('/').next().unwrap_or("index").to_string()
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

        dir_index_map.insert(dir_rel.clone(), index_rel);
    }

    // --- Phase 4: Compute attachments per directory ---
    let mut dir_attachments: HashMap<String, Vec<String>> = HashMap::new();
    for rel_path in &non_md_files {
        let dir_rel = parent_rel_path(rel_path);
        dir_attachments
            .entry(dir_rel)
            .or_default()
            .push(rel_path.clone());
    }

    // --- Phase 5: Update non-index markdown files (add part_of) ---
    for rel_path in &md_files {
        // Skip index files
        if is_index_file(rel_path, &dir_index_map) {
            continue;
        }

        let dir_rel = parent_rel_path(rel_path);
        let index_path = match dir_index_map.get(&dir_rel) {
            Some(p) => p.clone(),
            None => continue,
        };

        let full_path = join_path(root, rel_path);

        // Idempotency: skip if part_of is already set
        if let Ok(content) = fs.read_to_string(&full_path).await
            && let Ok(parsed) = frontmatter::parse_or_empty(&content)
            && parsed.frontmatter.contains_key("part_of")
        {
            result.skipped += 1;
            continue;
        }

        let metadata = serde_json::json!({
            "part_of": index_path,
        });

        match metadata_writer::update_file_metadata(fs, &full_path, &metadata, None).await {
            Ok(()) => {
                result.imported += 1;
            }
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to update {rel_path}: {e}"));
            }
        }
    }

    // --- Phase 6: Create missing index files ---
    for dir_rel in &all_dirs {
        let index_rel = match dir_index_map.get(dir_rel.as_str()) {
            Some(r) => r.clone(),
            None => continue,
        };

        // Skip if this index came from an existing source file
        if md_files.contains(&index_rel) {
            continue;
        }

        let full_path = join_path(root, &index_rel);

        // Idempotency: skip if file already exists
        if fs.exists(&full_path).await {
            result.skipped += 1;
            continue;
        }

        let metadata = build_index_metadata(
            dir_rel,
            &index_rel,
            &dir_index_map,
            &md_files,
            &dir_attachments,
            &all_dirs,
            root,
        );

        match metadata_writer::write_file_with_metadata(fs, &full_path, &metadata, "").await {
            Ok(()) => {
                result.imported += 1;
            }
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to create index {index_rel}: {e}"));
            }
        }
    }

    // --- Phase 7: Update existing source indexes (add part_of/contents/attachments) ---
    for (dir_rel, index_rel) in &dir_index_map {
        // Skip generated indexes (already handled in Phase 6)
        if !md_files.contains(index_rel) {
            continue;
        }

        let full_path = join_path(root, index_rel);

        // Check what's missing
        let (has_part_of, has_contents, has_attachments) = match fs.read_to_string(&full_path).await
        {
            Ok(content) => match frontmatter::parse_or_empty(&content) {
                Ok(parsed) => {
                    let fm = &parsed.frontmatter;
                    (
                        fm.contains_key("part_of"),
                        fm.contains_key("contents"),
                        fm.contains_key("attachments"),
                    )
                }
                Err(_) => (false, false, false),
            },
            Err(_) => continue,
        };

        // Skip if nothing to add
        let needs_part_of = !has_part_of && !dir_rel.is_empty();
        let needs_contents = !has_contents;
        let needs_attachments = !has_attachments && dir_attachments.contains_key(dir_rel.as_str());

        if !needs_part_of && !needs_contents && !needs_attachments {
            result.skipped += 1;
            continue;
        }

        let mut metadata = serde_json::Map::new();

        if needs_part_of {
            let parent_dir = parent_rel_path(dir_rel);
            if let Some(parent_index) = dir_index_map.get(&parent_dir) {
                metadata.insert(
                    "part_of".to_string(),
                    serde_json::Value::String(parent_index.clone()),
                );
            }
        }

        if needs_contents {
            let contents = collect_contents(dir_rel, &dir_index_map, &md_files, &all_dirs);
            if !contents.is_empty() {
                metadata.insert(
                    "contents".to_string(),
                    serde_json::Value::Array(
                        contents
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
            }
        }

        if needs_attachments
            && let Some(atts) = dir_attachments.get(dir_rel.as_str())
            && !atts.is_empty()
        {
            metadata.insert(
                "attachments".to_string(),
                serde_json::Value::Array(
                    atts.iter()
                        .map(|a| serde_json::Value::String(a.clone()))
                        .collect(),
                ),
            );
        }

        if metadata.is_empty() {
            result.skipped += 1;
            continue;
        }

        let json_value = serde_json::Value::Object(metadata);
        match metadata_writer::update_file_metadata(fs, &full_path, &json_value, None).await {
            Ok(()) => {
                result.imported += 1;
            }
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to update index {index_rel}: {e}"));
            }
        }
    }

    Ok(result)
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Recursively walk a directory, collecting markdown files, non-markdown files,
/// and directory paths.
async fn walk_dir<FS: AsyncFileSystem>(
    fs: &FS,
    root: &Path,
    current: &Path,
    md_files: &mut Vec<String>,
    non_md_files: &mut Vec<String>,
    directories: &mut HashSet<String>,
) {
    let entries = match fs.list_files(current).await {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut entries = entries;
    entries.sort();

    for entry in entries {
        let name = match entry.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }

        if fs.is_dir(&entry).await {
            // Skip build/dependency directories
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }

            let rel = relative_path(root, &entry);
            directories.insert(rel);

            Box::pin(walk_dir(
                fs,
                root,
                &entry,
                md_files,
                non_md_files,
                directories,
            ))
            .await;
        } else {
            let rel = relative_path(root, &entry);

            if name.ends_with(".md") || name.ends_with(".MD") {
                md_files.push(rel);
            } else {
                non_md_files.push(rel);
            }
        }
    }
}

/// Collect the `contents` entries for a directory's index file.
fn collect_contents(
    dir_rel: &str,
    dir_index_map: &IndexMap<String, String>,
    md_files: &[String],
    all_dirs: &[String],
) -> Vec<String> {
    let mut contents: Vec<String> = Vec::new();

    // Child markdown files (non-indexes) in this directory
    for rel_path in md_files {
        if parent_rel_path(rel_path) != dir_rel {
            continue;
        }
        if is_index_file(rel_path, dir_index_map) {
            continue;
        }
        contents.push(rel_path.clone());
    }

    // Child directory indexes
    for child_dir in all_dirs {
        if child_dir.is_empty() && !dir_rel.is_empty() {
            continue;
        }
        if child_dir == dir_rel {
            continue;
        }
        if parent_rel_path(child_dir) != dir_rel {
            continue;
        }
        if let Some(child_index) = dir_index_map.get(child_dir.as_str()) {
            contents.push(child_index.clone());
        }
    }

    contents
}

/// Build the metadata JSON for a new index file.
fn build_index_metadata(
    dir_rel: &str,
    _index_rel: &str,
    dir_index_map: &IndexMap<String, String>,
    md_files: &[String],
    dir_attachments: &HashMap<String, Vec<String>>,
    all_dirs: &[String],
    root: &Path,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();

    // title
    let dir_name = if dir_rel.is_empty() {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Index")
            .to_string()
    } else {
        dir_rel.rsplit('/').next().unwrap_or("Index").to_string()
    };
    let title = prettify_filename(&dir_name);
    metadata.insert("title".to_string(), serde_json::Value::String(title));

    // part_of (skip for root)
    if !dir_rel.is_empty() {
        let parent_dir = parent_rel_path(dir_rel);
        if let Some(parent_index) = dir_index_map.get(&parent_dir) {
            metadata.insert(
                "part_of".to_string(),
                serde_json::Value::String(parent_index.clone()),
            );
        }
    }

    // contents
    let contents = collect_contents(dir_rel, dir_index_map, md_files, all_dirs);
    if !contents.is_empty() {
        metadata.insert(
            "contents".to_string(),
            serde_json::Value::Array(
                contents
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }

    // attachments
    if let Some(atts) = dir_attachments.get(dir_rel)
        && !atts.is_empty()
    {
        metadata.insert(
            "attachments".to_string(),
            serde_json::Value::Array(
                atts.iter()
                    .map(|a| serde_json::Value::String(a.clone()))
                    .collect(),
            ),
        );
    }

    serde_json::Value::Object(metadata)
}

/// Get the parent relative path from a relative file path.
/// Returns "" for files at the root level.
fn parent_rel_path(rel_path: &str) -> String {
    match rel_path.rfind('/') {
        Some(idx) => rel_path[..idx].to_string(),
        None => String::new(),
    }
}

/// Check if a file is an index file for any directory.
fn is_index_file(rel_path: &str, dir_index_map: &IndexMap<String, String>) -> bool {
    dir_index_map.values().any(|v| v == rel_path)
}

/// Get the filename component of a path.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Compute a relative path from root to entry.
fn relative_path(root: &Path, entry: &Path) -> String {
    entry
        .strip_prefix(root)
        .unwrap_or(entry)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Join root path with a relative path string.
fn join_path(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}
