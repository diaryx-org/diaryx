//! Workspace materialization — extract CRDT state into files.
//!
//! This module extracts the current workspace state from CRDT documents into
//! a list of `MaterializedFile` values. Callers can then write these to ZIP,
//! git trees, or the filesystem.
//!
//! The primary entry point is [`materialize_workspace`].

use std::collections::HashMap;

use super::body_doc_manager::BodyDocManager;
use super::types::FileMetadata;
use super::workspace_doc::WorkspaceCrdt;
use crate::metadata_writer::FrontmatterMetadata;

/// A single file extracted from CRDT state.
#[derive(Debug, Clone)]
pub struct MaterializedFile {
    /// Workspace-relative path (e.g. `Daily/2024/note.md`).
    pub path: String,
    /// Full file content (frontmatter + body).
    pub content: String,
    /// The raw metadata from the workspace CRDT.
    pub metadata: FileMetadata,
}

/// Result of a materialization run.
#[derive(Debug)]
pub struct MaterializationResult {
    /// Successfully materialized files.
    pub files: Vec<MaterializedFile>,
    /// Doc-IDs that were skipped (unresolved path, deleted, etc.)
    pub skipped: Vec<SkippedFile>,
}

/// A file that was skipped during materialization.
#[derive(Debug, Clone)]
pub struct SkippedFile {
    /// The doc-ID (or path key) in the workspace CRDT.
    pub key: String,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// Reason a file was skipped during materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The file is soft-deleted.
    Deleted,
    /// The path could not be resolved from the doc-ID chain.
    UnresolvedPath,
}

/// Materialize the current workspace state into a list of files.
///
/// This is the shared logic behind `export_snapshot_zip` (server) and
/// `commit_workspace` (git). It walks the workspace CRDT's file index,
/// resolves each doc-ID to a filesystem path, reads the corresponding body
/// document, and assembles frontmatter + body into complete file content.
///
/// # Arguments
///
/// * `workspace` — The workspace CRDT to materialize.
/// * `body_docs` — Manager for per-file body CRDTs.
/// * `workspace_id` — The workspace identifier (used to construct body doc keys).
pub fn materialize_workspace(
    workspace: &WorkspaceCrdt,
    body_docs: &BodyDocManager,
    workspace_id: &str,
) -> MaterializationResult {
    let files_list = workspace.list_files();

    // Build doc-ID → path lookup table.
    let mut id_to_path: HashMap<String, String> = HashMap::new();
    for (key, _meta) in &files_list {
        if key.contains('/') || key.ends_with(".md") {
            // Legacy path-based key — it *is* the path.
            id_to_path.insert(key.clone(), key.clone());
        } else if let Some(path) = workspace.get_path(key) {
            id_to_path.insert(key.clone(), path.to_string_lossy().to_string());
        }
    }

    let mut result_files = Vec::new();
    let mut skipped = Vec::new();

    for (key, meta) in files_list {
        if meta.deleted {
            skipped.push(SkippedFile {
                key,
                reason: SkipReason::Deleted,
            });
            continue;
        }

        let path = match resolve_path(&key, &id_to_path) {
            Some(p) => p,
            None => {
                skipped.push(SkippedFile {
                    key,
                    reason: SkipReason::UnresolvedPath,
                });
                continue;
            }
        };

        // Resolve part_of / contents from doc-IDs to paths for export.
        let mut export_meta = meta.clone();
        export_meta.part_of = export_meta
            .part_of
            .and_then(|value| resolve_path(&value, &id_to_path));

        if let Some(contents) = export_meta.contents.take() {
            let resolved: Vec<String> = contents
                .into_iter()
                .filter_map(|value| resolve_path(&value, &id_to_path))
                .collect();
            export_meta.contents = Some(resolved);
        }

        // Build frontmatter YAML.
        let metadata_json = serde_json::to_value(&export_meta).unwrap_or_default();
        let fm = FrontmatterMetadata::from_json_with_file_path(&metadata_json, Some(&path));
        let yaml = fm.to_yaml();

        // Read body content.
        let body_key = format!("body:{}/{}", workspace_id, path);
        let mut body = body_docs.get_or_create(&body_key).get_body();
        if body.is_empty() && key != path {
            let alt_key = format!("body:{}/{}", workspace_id, key);
            body = body_docs.get_or_create(&alt_key).get_body();
        }

        let content = if yaml.is_empty() {
            body
        } else {
            format!("---\n{}\n---\n\n{}", yaml, body)
        };

        result_files.push(MaterializedFile {
            path,
            content,
            metadata: meta,
        });
    }

    MaterializationResult {
        files: result_files,
        skipped,
    }
}

/// Resolve a doc-ID or path-key to a filesystem path.
fn resolve_path(value: &str, id_to_path: &HashMap<String, String>) -> Option<String> {
    if value.contains('/') || value.ends_with(".md") {
        Some(value.to_string())
    } else {
        id_to_path.get(value).cloned()
    }
}

/// Parse a markdown file with optional YAML frontmatter into metadata + body.
///
/// This is extracted from `store.rs::parse_snapshot_markdown` so it can be
/// reused by the git rebuild path.
pub fn parse_snapshot_markdown(
    path: &str,
    content: &str,
) -> Result<(FileMetadata, String), String> {
    let parsed = crate::frontmatter::parse_or_empty(content)
        .map_err(|e| format!("Failed to parse frontmatter in {}: {}", path, e))?;
    let fm = &parsed.frontmatter;
    let body = parsed.body;
    let file_path = std::path::Path::new(path);

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let part_of = fm.get("part_of").and_then(|v| v.as_str()).map(|raw| {
        let parsed_link = crate::link_parser::parse_link(raw);
        crate::link_parser::to_canonical(&parsed_link, file_path)
    });

    let contents = fm.get("contents").and_then(|v| {
        v.as_sequence().map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str())
                .map(|raw| {
                    let parsed_link = crate::link_parser::parse_link(raw);
                    crate::link_parser::to_canonical(&parsed_link, file_path)
                })
                .collect::<Vec<String>>()
        })
    });

    let audience = fm.get("audience").and_then(|v| {
        v.as_sequence().map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
    });

    let description = fm
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);

    let attachments = fm
        .get("attachments")
        .and_then(|v| {
            v.as_sequence().map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str())
                    .map(|raw| {
                        let parsed_link = crate::link_parser::parse_link(raw);
                        let canonical = crate::link_parser::to_canonical(&parsed_link, file_path);
                        super::types::BinaryRef {
                            path: canonical,
                            source: "local".to_string(),
                            hash: String::new(),
                            mime_type: String::new(),
                            size: 0,
                            uploaded_at: None,
                            deleted: false,
                        }
                    })
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default();

    let modified_at = fm
        .get("updated")
        .and_then(parse_updated_value)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    let metadata = FileMetadata {
        filename,
        title: fm.get("title").and_then(|v| v.as_str()).map(String::from),
        part_of,
        contents,
        attachments,
        deleted: fm.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false),
        audience,
        description,
        extra: HashMap::new(),
        modified_at,
    };

    Ok((metadata, body))
}

fn parse_updated_value(value: &serde_yaml::Value) -> Option<i64> {
    if let Some(num) = value.as_i64() {
        return Some(num);
    }
    if let Some(num) = value.as_f64() {
        return Some(num as i64);
    }
    if let Some(raw) = value.as_str() {
        if let Ok(num) = raw.parse::<i64>() {
            return Some(num);
        }
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(raw) {
            return Some(parsed.timestamp_millis());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;
    use std::sync::Arc;

    #[test]
    fn test_materialize_empty_workspace() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage);

        let result = materialize_workspace(&workspace, &body_docs, "test-ws");
        assert!(result.files.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn test_materialize_single_file() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage);

        let meta = FileMetadata::with_filename("hello.md".to_string(), Some("Hello".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();

        let body_key = format!(
            "body:test-ws/{}",
            workspace.get_path(&doc_id).unwrap().to_string_lossy()
        );
        body_docs
            .get_or_create(&body_key)
            .set_body("Hello world")
            .unwrap();

        let result = materialize_workspace(&workspace, &body_docs, "test-ws");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "hello.md");
        assert!(result.files[0].content.contains("Hello world"));
        assert!(result.files[0].content.contains("title: Hello"));
    }

    #[test]
    fn test_materialize_skips_deleted() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage);

        let mut meta =
            FileMetadata::with_filename("deleted.md".to_string(), Some("Gone".to_string()));
        meta.mark_deleted();
        workspace.create_file(meta).unwrap();

        let result = materialize_workspace(&workspace, &body_docs, "test-ws");
        assert!(result.files.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].reason, SkipReason::Deleted);
    }

    #[test]
    fn test_materialize_nested_path() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage);

        // Create parent index
        let parent_meta =
            FileMetadata::with_filename("daily".to_string(), Some("Daily".to_string()));
        let parent_id = workspace.create_file(parent_meta).unwrap();

        // Create child
        let mut child_meta =
            FileMetadata::with_filename("note.md".to_string(), Some("Note".to_string()));
        child_meta.part_of = Some(parent_id.clone());
        let child_id = workspace.create_file(child_meta).unwrap();

        let child_path = workspace.get_path(&child_id).unwrap();
        assert_eq!(child_path.to_string_lossy(), "daily/note.md");

        let body_key = format!("body:test-ws/{}", child_path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Daily note content")
            .unwrap();

        let result = materialize_workspace(&workspace, &body_docs, "test-ws");
        // Should have parent and child
        let child_file = result.files.iter().find(|f| f.path == "daily/note.md");
        assert!(child_file.is_some());
        assert!(child_file.unwrap().content.contains("Daily note content"));
    }

    #[test]
    fn test_parse_snapshot_markdown_with_frontmatter() {
        let content = "---\ntitle: Test\nupdated: 1700000000000\n---\n\nBody text here";
        let (meta, body) = parse_snapshot_markdown("test.md", content).unwrap();
        assert_eq!(meta.title, Some("Test".to_string()));
        assert_eq!(meta.modified_at, 1700000000000);
        assert!(body.contains("Body text here"));
        assert_eq!(meta.filename, "test.md");
    }

    #[test]
    fn test_parse_snapshot_markdown_no_frontmatter() {
        let content = "Just body text";
        let (meta, body) = parse_snapshot_markdown("note.md", content).unwrap();
        assert!(meta.title.is_none());
        assert_eq!(body, "Just body text");
        assert_eq!(meta.filename, "note.md");
    }

    #[test]
    fn test_resolve_path_with_path_key() {
        let map = HashMap::new();
        assert_eq!(
            resolve_path("folder/file.md", &map),
            Some("folder/file.md".to_string())
        );
        assert_eq!(resolve_path("file.md", &map), Some("file.md".to_string()));
    }

    #[test]
    fn test_resolve_path_with_doc_id() {
        let mut map = HashMap::new();
        map.insert("abc-123".to_string(), "folder/file.md".to_string());
        assert_eq!(
            resolve_path("abc-123", &map),
            Some("folder/file.md".to_string())
        );
        assert_eq!(resolve_path("unknown-id", &map), None);
    }
}
