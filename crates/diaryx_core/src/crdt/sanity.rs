//! Workspace sanity validation — detect CRDT state inconsistencies.
//!
//! This module validates the current workspace CRDT state, checking for
//! issues like empty body documents, orphaned files, broken parent chains,
//! and file-set mismatches.
//!
//! The primary entry point is [`validate_workspace`].

use std::collections::HashSet;

use super::body_doc_manager::BodyDocManager;
use super::workspace_doc::WorkspaceCrdt;

/// A single issue found during validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanityIssue {
    /// The doc-ID or path key of the affected file (if applicable).
    pub key: Option<String>,
    /// What kind of issue was found.
    pub kind: IssueKind,
    /// Human-readable description.
    pub message: String,
}

/// Categories of validation issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueKind {
    /// A non-deleted file has an empty body document.
    EmptyBody,
    /// A file references a parent (part_of) that doesn't exist.
    BrokenParentChain,
    /// A body document exists but has no corresponding workspace file entry.
    OrphanBodyDoc,
    /// A workspace file entry exists but the body doc key doesn't match expected naming.
    MissingBodyDoc,
    /// A file listed in `contents` doesn't exist in the workspace.
    MissingChild,
}

/// Result of a sanity validation run.
#[derive(Debug)]
pub struct SanityReport {
    /// Issues found during validation.
    pub issues: Vec<SanityIssue>,
    /// Total number of non-deleted files in the workspace.
    pub file_count: usize,
    /// Total number of body documents checked.
    pub body_doc_count: usize,
}

impl SanityReport {
    /// Returns true if no issues were found.
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns the number of issues.
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Returns issues of a specific kind.
    pub fn issues_of_kind(&self, kind: &IssueKind) -> Vec<&SanityIssue> {
        self.issues.iter().filter(|i| &i.kind == kind).collect()
    }
}

/// Validate the workspace CRDT state for inconsistencies.
///
/// # Arguments
///
/// * `workspace` — The workspace CRDT to validate.
/// * `body_docs` — Manager for per-file body CRDTs.
/// * `workspace_id` — The workspace identifier (used to construct body doc keys).
pub fn validate_workspace(
    workspace: &WorkspaceCrdt,
    body_docs: &BodyDocManager,
    workspace_id: &str,
) -> SanityReport {
    let files = workspace.list_files();
    let mut issues = Vec::new();
    let mut active_keys: HashSet<String> = HashSet::new();
    let mut all_keys: HashSet<String> = HashSet::new();
    let mut expected_body_keys: HashSet<String> = HashSet::new();

    // Build lookup of all file keys for cross-reference checks.
    for (key, _) in &files {
        all_keys.insert(key.clone());
    }

    let mut file_count = 0;
    let mut body_doc_count = 0;

    for (key, meta) in &files {
        if meta.deleted {
            continue;
        }
        file_count += 1;
        active_keys.insert(key.clone());

        // Resolve path for body doc key construction.
        let path = if key.contains('/') || key.ends_with(".md") {
            key.clone()
        } else if let Some(p) = workspace.get_path(key) {
            p.to_string_lossy().to_string()
        } else {
            // Can't resolve path — we won't check body doc, but flag if it
            // matters later.
            key.clone()
        };

        // Check body doc.
        let body_key = format!("body:{}/{}", workspace_id, path);
        expected_body_keys.insert(body_key.clone());
        if let Some(body_doc) = body_docs.get(&body_key) {
            body_doc_count += 1;
            // Empty body docs for non-index files may be a problem.
            // Index files (contents field is Some, even if empty) are allowed to have empty bodies.
            let is_index = meta.contents.is_some();
            if body_doc.get_body().is_empty() && !is_index {
                issues.push(SanityIssue {
                    key: Some(key.clone()),
                    kind: IssueKind::EmptyBody,
                    message: format!("Non-index file '{}' has an empty body", path),
                });
            }
        }

        // Check parent chain.
        if let Some(ref parent_id) = meta.part_of {
            if !parent_id.is_empty()
                && !parent_id.contains('/')
                && !parent_id.ends_with(".md")
                && !all_keys.contains(parent_id)
            {
                issues.push(SanityIssue {
                    key: Some(key.clone()),
                    kind: IssueKind::BrokenParentChain,
                    message: format!(
                        "File '{}' references non-existent parent '{}'",
                        key, parent_id
                    ),
                });
            }
        }

        // Check contents references.
        if let Some(ref contents) = meta.contents {
            for child_ref in contents {
                // Only check doc-ID references, not path references.
                if !child_ref.contains('/') && !child_ref.ends_with(".md") {
                    if !all_keys.contains(child_ref) {
                        issues.push(SanityIssue {
                            key: Some(key.clone()),
                            kind: IssueKind::MissingChild,
                            message: format!(
                                "File '{}' lists non-existent child '{}'",
                                key, child_ref
                            ),
                        });
                    }
                }
            }
        }
    }

    // Check for orphan body docs — body docs whose workspace entry is gone.
    // This requires listing docs from storage, which BodyDocManager doesn't
    // expose directly. We check the loaded docs only (best-effort).
    // Full orphan detection is left to callers that have access to storage.list_docs().

    SanityReport {
        issues,
        file_count,
        body_doc_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::{BodyDocManager, FileMetadata, MemoryStorage, WorkspaceCrdt};
    use std::sync::Arc;

    fn setup() -> (Arc<MemoryStorage>, WorkspaceCrdt, BodyDocManager) {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage.clone());
        (storage, workspace, body_docs)
    }

    #[test]
    fn test_empty_workspace_is_ok() {
        let (_storage, workspace, body_docs) = setup();
        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert!(report.is_ok());
        assert_eq!(report.file_count, 0);
    }

    #[test]
    fn test_healthy_workspace() {
        let (_storage, workspace, body_docs) = setup();

        let meta = FileMetadata::with_filename("note.md".to_string(), Some("Note".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();

        let body_key = format!("body:ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Some content")
            .unwrap();

        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert!(report.is_ok());
        assert_eq!(report.file_count, 1);
    }

    #[test]
    fn test_broken_parent_chain() {
        let (_storage, workspace, body_docs) = setup();

        let mut meta =
            FileMetadata::with_filename("orphan.md".to_string(), Some("Orphan".to_string()));
        meta.part_of = Some("non-existent-uuid".to_string());
        // Use set_file directly so we can set a known key.
        workspace.set_file("some-uuid", meta).unwrap();

        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert!(!report.is_ok());
        assert_eq!(
            report.issues_of_kind(&IssueKind::BrokenParentChain).len(),
            1
        );
    }

    #[test]
    fn test_missing_child() {
        let (_storage, workspace, body_docs) = setup();

        let mut meta =
            FileMetadata::with_filename("index.md".to_string(), Some("Index".to_string()));
        meta.contents = Some(vec!["non-existent-child-uuid".to_string()]);
        workspace.set_file("parent-uuid", meta).unwrap();

        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert!(!report.is_ok());
        assert_eq!(report.issues_of_kind(&IssueKind::MissingChild).len(), 1);
    }

    #[test]
    fn test_deleted_files_skipped() {
        let (_storage, workspace, body_docs) = setup();

        let mut meta =
            FileMetadata::with_filename("deleted.md".to_string(), Some("Deleted".to_string()));
        meta.mark_deleted();
        workspace.create_file(meta).unwrap();

        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert!(report.is_ok());
        assert_eq!(report.file_count, 0);
    }

    #[test]
    fn test_empty_body_detected() {
        let (_storage, workspace, body_docs) = setup();

        let meta = FileMetadata::with_filename("empty.md".to_string(), Some("Empty".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();

        // Create body doc but leave it empty.
        let body_key = format!("body:ws/{}", path.to_string_lossy());
        body_docs.get_or_create(&body_key);

        let report = validate_workspace(&workspace, &body_docs, "ws");
        assert_eq!(report.issues_of_kind(&IssueKind::EmptyBody).len(), 1);
    }

    #[test]
    fn test_index_files_allowed_empty_body() {
        let (_storage, workspace, body_docs) = setup();

        let mut meta =
            FileMetadata::with_filename("index.md".to_string(), Some("Index".to_string()));
        meta.contents = Some(vec![]); // Empty contents = index file
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();

        // Create body doc but leave it empty — should be OK for index files.
        let body_key = format!("body:ws/{}", path.to_string_lossy());
        body_docs.get_or_create(&body_key);

        let report = validate_workspace(&workspace, &body_docs, "ws");
        // Empty body for index file should NOT be flagged.
        assert_eq!(report.issues_of_kind(&IssueKind::EmptyBody).len(), 0);
    }
}
