//! In-place directory import: convert a directory of files to Diaryx
//! hierarchy format by adding `part_of`/`contents`/`attachments` frontmatter.
//!
//! Port of `diaryx_core::import::directory::import_directory_in_place` using
//! host bridge calls instead of `AsyncFileSystem`.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use diaryx_core::entry::prettify_filename;
use diaryx_core::frontmatter;
use diaryx_core::import::ImportResult;

use crate::host_bridge;

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
/// Uses `host_list_files` to enumerate all files, then derives directory
/// structure from paths. Adds `part_of`/`contents`/`attachments` frontmatter.
///
/// This operation is idempotent: files that already have correct metadata are
/// skipped. Running it twice produces the same result.
pub fn import_directory_in_place(root: &str) -> Result<ImportResult, String> {
    let mut result = ImportResult {
        imported: 0,
        skipped: 0,
        errors: Vec::new(),
        attachment_count: 0,
    };

    // --- Phase 1: Walk & classify ---
    // Use host_list_files to get all files, then derive directory structure.
    let all_files = host_bridge::list_files(root)?;

    let mut md_files: Vec<String> = Vec::new();
    let mut non_md_files: Vec<String> = Vec::new();
    let mut directories: HashSet<String> = HashSet::new();
    directories.insert(String::new()); // root directory

    for file_path in &all_files {
        // Compute relative path from root
        let rel = if root.is_empty() {
            file_path.clone()
        } else if let Some(stripped) = file_path.strip_prefix(root) {
            stripped.trim_start_matches('/').to_string()
        } else {
            file_path.clone()
        };

        if rel.is_empty() {
            continue;
        }

        // Skip hidden files/dirs
        if rel.split('/').any(|seg| seg.starts_with('.')) {
            continue;
        }

        // Skip known build/dependency directories
        if rel.split('/').any(|seg| SKIP_DIRS.contains(&seg)) {
            continue;
        }

        // Register all parent directories
        let mut current = String::new();
        for segment in rel
            .split('/')
            .take(rel.split('/').count().saturating_sub(1))
        {
            if current.is_empty() {
                current = segment.to_string();
            } else {
                current = format!("{current}/{segment}");
            }
            directories.insert(current.clone());
        }

        let name = rel.rsplit('/').next().unwrap_or(&rel);
        if name.ends_with(".md") || name.ends_with(".MD") {
            md_files.push(rel);
        } else {
            non_md_files.push(rel);
        }
    }

    if md_files.is_empty() && non_md_files.is_empty() {
        return Ok(result);
    }

    // --- Phase 2: Detect existing indexes ---
    let mut dir_index_map: IndexMap<String, String> = IndexMap::new();

    for rel_path in &md_files {
        let filename = file_name(rel_path);
        let dir_rel = parent_rel_path(rel_path);

        let is_index_by_name = filename == "index.md" || filename.ends_with("_index.md");

        let is_index_by_contents = if !is_index_by_name {
            let full_path = join_path(root, rel_path);
            match host_bridge::read_file(&full_path) {
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
            if root.is_empty() {
                "index".to_string()
            } else {
                root.rsplit('/').next().unwrap_or("index").to_string()
            }
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
        if let Ok(content) = host_bridge::read_file(&full_path)
            && let Ok(parsed) = frontmatter::parse_or_empty(&content)
        {
            if parsed.frontmatter.contains_key("part_of") {
                result.skipped += 1;
                continue;
            }

            // Add part_of
            let mut fm = parsed.frontmatter;
            fm.insert("part_of".to_string(), serde_yaml::Value::String(index_path));

            if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
                match host_bridge::write_file(&full_path, &updated) {
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
        if host_bridge::file_exists(&full_path).unwrap_or(false) {
            result.skipped += 1;
            continue;
        }

        let metadata = build_index_metadata(
            dir_rel,
            &dir_index_map,
            &md_files,
            &dir_attachments,
            &all_dirs,
            root,
        );

        let content = format_metadata_as_markdown(&metadata);
        match host_bridge::write_file(&full_path, &content) {
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
        let (has_part_of, has_contents, has_attachments) = match host_bridge::read_file(&full_path)
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

        let needs_part_of = !has_part_of && !dir_rel.is_empty();
        let needs_contents = !has_contents;
        let needs_attachments = !has_attachments && dir_attachments.contains_key(dir_rel.as_str());

        if !needs_part_of && !needs_contents && !needs_attachments {
            result.skipped += 1;
            continue;
        }

        // Re-read file to update
        let content = match host_bridge::read_file(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parsed = match frontmatter::parse_or_empty(&content) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut fm = parsed.frontmatter;

        if needs_part_of {
            let parent_dir = parent_rel_path(dir_rel);
            if let Some(parent_index) = dir_index_map.get(&parent_dir) {
                fm.insert(
                    "part_of".to_string(),
                    serde_yaml::Value::String(parent_index.clone()),
                );
            }
        }

        if needs_contents {
            let contents = collect_contents(dir_rel, &dir_index_map, &md_files, &all_dirs);
            if !contents.is_empty() {
                fm.insert(
                    "contents".to_string(),
                    serde_yaml::Value::Sequence(
                        contents
                            .into_iter()
                            .map(serde_yaml::Value::String)
                            .collect(),
                    ),
                );
            }
        }

        if needs_attachments {
            if let Some(atts) = dir_attachments.get(dir_rel.as_str()) {
                if !atts.is_empty() {
                    fm.insert(
                        "attachments".to_string(),
                        serde_yaml::Value::Sequence(
                            atts.iter()
                                .map(|a| serde_yaml::Value::String(a.clone()))
                                .collect(),
                        ),
                    );
                }
            }
        }

        if let Ok(updated) = frontmatter::serialize(&fm, &parsed.body) {
            match host_bridge::write_file(&full_path, &updated) {
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
    }

    Ok(result)
}

// ── Helpers ──────────────────────────────────────────────────────────────

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

/// Build the metadata for a new index file.
fn build_index_metadata(
    dir_rel: &str,
    dir_index_map: &IndexMap<String, String>,
    md_files: &[String],
    dir_attachments: &HashMap<String, Vec<String>>,
    all_dirs: &[String],
    root: &str,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();

    // title
    let dir_name = if dir_rel.is_empty() {
        if root.is_empty() {
            "Index".to_string()
        } else {
            root.rsplit('/').next().unwrap_or("Index").to_string()
        }
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
    if let Some(atts) = dir_attachments.get(dir_rel) {
        if !atts.is_empty() {
            metadata.insert(
                "attachments".to_string(),
                serde_json::Value::Array(
                    atts.iter()
                        .map(|a| serde_json::Value::String(a.clone()))
                        .collect(),
                ),
            );
        }
    }

    serde_json::Value::Object(metadata)
}

/// Format metadata JSON as a markdown file with frontmatter.
fn format_metadata_as_markdown(metadata: &serde_json::Value) -> String {
    // Convert JSON to YAML frontmatter
    let obj = match metadata.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    let mut fm = IndexMap::new();
    for (key, value) in obj {
        if let Ok(yaml_val) = serde_json::from_value::<serde_yaml::Value>(value.clone()) {
            fm.insert(key.clone(), yaml_val);
        }
    }

    frontmatter::serialize(&fm, "").unwrap_or_else(|_| {
        let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("Index");
        format!("---\ntitle: {title}\n---\n")
    })
}

/// Get the parent relative path from a relative file path.
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

/// Join root path with a relative path string.
fn join_path(root: &str, rel: &str) -> String {
    if root.is_empty() {
        rel.to_string()
    } else if rel.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{rel}")
    }
}
