//! Git commit workflow — materialize, validate, commit, compact.

use std::sync::Arc;

use git2::{Oid, Repository, Signature};

use crate::crdt::body_doc_manager::BodyDocManager;
use crate::crdt::materialize::materialize_workspace;
use crate::crdt::sanity::validate_workspace;
use crate::crdt::self_healing::{HealingAction, HealthTracker};
use crate::crdt::storage::CrdtStorage;
use crate::crdt::workspace_doc::WorkspaceCrdt;
use crate::error::DiaryxError;

/// Options for a workspace commit.
pub struct CommitOptions {
    /// Commit message. Defaults to an auto-generated timestamp message.
    pub message: Option<String>,
    /// Author name for the commit.
    pub author_name: String,
    /// Author email for the commit.
    pub author_email: String,
    /// Number of CRDT updates to keep after compaction. 0 = compact all.
    pub keep_updates: usize,
    /// Whether to skip validation before committing.
    pub skip_validation: bool,
}

impl Default for CommitOptions {
    fn default() -> Self {
        Self {
            message: None,
            author_name: "Diaryx".to_string(),
            author_email: "noreply@diaryx.app".to_string(),
            keep_updates: 0,
            skip_validation: false,
        }
    }
}

/// Result of a successful commit.
#[derive(Debug)]
pub struct CommitResult {
    /// The git commit OID.
    pub commit_id: Oid,
    /// Number of files in the commit tree.
    pub file_count: usize,
    /// Whether compaction was performed.
    pub compacted: bool,
}

/// Materialize workspace state, validate, build a git commit, then compact CRDT.
///
/// # Workflow
/// 1. Materialize workspace → list of files
/// 2. Validate (unless skipped) → check for issues
/// 3. Build git tree in-memory → handles nested dirs
/// 4. Create commit → pointing to tree
/// 5. Compact CRDT docs → remove old updates
///
/// # Arguments
///
/// * `storage` — CRDT storage backend (for compaction).
/// * `workspace` — Workspace CRDT to materialize.
/// * `body_docs` — Body document manager.
/// * `repo` — Git repository to commit into.
/// * `workspace_id` — Workspace identifier.
/// * `options` — Commit configuration.
/// * `health_tracker` — Tracks validation failures for self-healing.
pub fn commit_workspace(
    storage: &Arc<dyn CrdtStorage>,
    workspace: &WorkspaceCrdt,
    body_docs: &BodyDocManager,
    repo: &Repository,
    workspace_id: &str,
    options: &CommitOptions,
    health_tracker: &mut HealthTracker,
) -> Result<CommitResult, DiaryxError> {
    // Step 1: Materialize
    let materialized = materialize_workspace(workspace, body_docs, workspace_id);

    if materialized.files.is_empty() {
        return Err(DiaryxError::Git("No files to commit".to_string()));
    }

    // Step 2: Validate
    if !options.skip_validation {
        let report = validate_workspace(workspace, body_docs, workspace_id);
        if !report.is_ok() {
            let action = health_tracker.record_failure();
            let issue_summary: Vec<String> = report
                .issues
                .iter()
                .take(5)
                .map(|i| i.message.clone())
                .collect();
            match action {
                HealingAction::SkipCommit => {
                    return Err(DiaryxError::Git(format!(
                        "Validation failed (attempt {}), skipping commit: {}",
                        health_tracker.consecutive_failures(),
                        issue_summary.join("; ")
                    )));
                }
                HealingAction::RebuildCrdt => {
                    return Err(DiaryxError::Git(format!(
                        "Validation failed {} times, CRDT rebuild recommended: {}",
                        health_tracker.consecutive_failures(),
                        issue_summary.join("; ")
                    )));
                }
                HealingAction::Proceed => {} // unreachable after failure
            }
        } else {
            health_tracker.record_success();
        }
    }

    // Step 3: Build git tree
    let tree_oid = build_tree(repo, &materialized.files)?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| DiaryxError::Git(e.to_string()))?;

    // Step 4: Create commit
    let sig = Signature::now(&options.author_name, &options.author_email)
        .map_err(|e| DiaryxError::Git(e.to_string()))?;

    let message = options.message.clone().unwrap_or_else(|| {
        let now = chrono::Utc::now();
        format!(
            "Workspace snapshot at {}",
            now.format("%Y-%m-%d %H:%M:%S UTC")
        )
    });

    let parent_commit = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

    let commit_oid = repo
        .commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)
        .map_err(|e| DiaryxError::Git(e.to_string()))?;

    let file_count = materialized.files.len();

    // Step 5: Compact
    let compacted = compact_workspace(
        storage,
        workspace,
        body_docs,
        workspace_id,
        options.keep_updates,
    )?;

    Ok(CommitResult {
        commit_id: commit_oid,
        file_count,
        compacted,
    })
}

/// Compact all CRDT documents for a workspace.
///
/// Compacts the workspace document and all body documents.
pub fn compact_workspace(
    storage: &Arc<dyn CrdtStorage>,
    workspace: &WorkspaceCrdt,
    body_docs: &BodyDocManager,
    workspace_id: &str,
    keep_updates: usize,
) -> Result<bool, DiaryxError> {
    // Compact workspace doc
    storage.compact(workspace.doc_name(), keep_updates)?;

    // Compact all body docs
    let files = workspace.list_files();
    for (key, meta) in &files {
        if meta.deleted {
            continue;
        }

        let path = if key.contains('/') || key.ends_with(".md") {
            key.clone()
        } else if let Some(p) = workspace.get_path(key) {
            p.to_string_lossy().to_string()
        } else {
            continue;
        };

        let body_key = format!("body:{}/{}", workspace_id, path);
        // Check if body doc exists in storage before compacting
        if body_docs.get(&body_key).is_some() {
            storage.compact(&body_key, keep_updates)?;
        }
    }

    Ok(true)
}

/// A materialized file reference used for tree building.
use crate::crdt::materialize::MaterializedFile;

/// Build a git tree from materialized files, handling nested directories.
fn build_tree(repo: &Repository, files: &[MaterializedFile]) -> Result<Oid, DiaryxError> {
    // Collect all files as (path_components, content) tuples.
    let mut entries: Vec<(Vec<&str>, &[u8])> = Vec::new();
    for file in files {
        let components: Vec<&str> = file.path.split('/').collect();
        entries.push((components, file.content.as_bytes()));
    }

    build_tree_recursive(repo, &entries, 0)
}

/// Recursively build nested git trees.
fn build_tree_recursive(
    repo: &Repository,
    entries: &[(Vec<&str>, &[u8])],
    depth: usize,
) -> Result<Oid, DiaryxError> {
    let mut builder = repo
        .treebuilder(None)
        .map_err(|e| DiaryxError::Git(e.to_string()))?;

    // Group entries by their component at the current depth.
    let mut dirs: std::collections::HashMap<&str, Vec<(Vec<&str>, &[u8])>> =
        std::collections::HashMap::new();

    for (components, content) in entries {
        if depth + 1 == components.len() {
            // This is a file at the current level — write blob.
            let blob_oid = repo
                .blob(content)
                .map_err(|e| DiaryxError::Git(e.to_string()))?;
            builder
                .insert(components[depth], blob_oid, 0o100644)
                .map_err(|e| DiaryxError::Git(e.to_string()))?;
        } else if depth < components.len() {
            // This entry is in a subdirectory.
            dirs.entry(components[depth])
                .or_default()
                .push((components.clone(), *content));
        }
    }

    // Recursively build subdirectory trees.
    for (dir_name, sub_entries) in &dirs {
        let sub_tree_oid = build_tree_recursive(repo, sub_entries, depth + 1)?;
        builder
            .insert(dir_name, sub_tree_oid, 0o040000)
            .map_err(|e| DiaryxError::Git(e.to_string()))?;
    }

    builder.write().map_err(|e| DiaryxError::Git(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::git::repo::{RepoKind, init_repo};
    use crate::crdt::{FileMetadata, MemoryStorage};

    fn setup() -> (
        Arc<MemoryStorage>,
        WorkspaceCrdt,
        BodyDocManager,
        tempfile::TempDir,
        Repository,
    ) {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage.clone());
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();
        (storage, workspace, body_docs, dir, repo)
    }

    #[test]
    fn test_commit_single_file() {
        let (storage, workspace, body_docs, _dir, repo) = setup();

        let meta = FileMetadata::with_filename("hello.md".to_string(), Some("Hello".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();

        let body_key = format!("body:test-ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Hello world")
            .unwrap();

        let mut tracker = HealthTracker::new();
        let result = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &CommitOptions::default(),
            &mut tracker,
        )
        .unwrap();

        assert_eq!(result.file_count, 1);
        assert!(result.compacted);

        // Verify git tree
        let commit = repo.find_commit(result.commit_id).unwrap();
        let tree = commit.tree().unwrap();
        assert!(tree.get_name("hello.md").is_some());
    }

    #[test]
    fn test_commit_nested_dirs() {
        let (storage, workspace, body_docs, _dir, repo) = setup();

        // Create parent directory (index file with contents)
        let mut parent_meta =
            FileMetadata::with_filename("daily".to_string(), Some("Daily".to_string()));
        parent_meta.contents = Some(vec![]); // Mark as index so empty body is OK
        let parent_id = workspace.create_file(parent_meta).unwrap();

        // Create nested file
        let mut child_meta =
            FileMetadata::with_filename("2024-01-01.md".to_string(), Some("Jan 1".to_string()));
        child_meta.part_of = Some(parent_id.clone());
        let child_id = workspace.create_file(child_meta).unwrap();
        let child_path = workspace.get_path(&child_id).unwrap();

        let body_key = format!("body:test-ws/{}", child_path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("New year content")
            .unwrap();

        let mut tracker = HealthTracker::new();
        let result = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &CommitOptions::default(),
            &mut tracker,
        )
        .unwrap();

        // Verify nested tree structure
        let commit = repo.find_commit(result.commit_id).unwrap();
        let tree = commit.tree().unwrap();
        let daily_entry = tree.get_name("daily").unwrap();
        let daily_tree = repo.find_tree(daily_entry.id()).unwrap();
        assert!(daily_tree.get_name("2024-01-01.md").is_some());
    }

    #[test]
    fn test_commit_empty_workspace_fails() {
        let (storage, workspace, body_docs, _dir, repo) = setup();

        let mut tracker = HealthTracker::new();
        let result = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &CommitOptions::default(),
            &mut tracker,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_commit_deleted_files_excluded() {
        let (storage, workspace, body_docs, _dir, repo) = setup();

        // Create a live file
        let meta = FileMetadata::with_filename("live.md".to_string(), Some("Live".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();
        let body_key = format!("body:test-ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Live content")
            .unwrap();

        // Create and delete a file
        let mut del_meta =
            FileMetadata::with_filename("deleted.md".to_string(), Some("Deleted".to_string()));
        del_meta.mark_deleted();
        workspace.create_file(del_meta).unwrap();

        let mut tracker = HealthTracker::new();
        let result = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &CommitOptions::default(),
            &mut tracker,
        )
        .unwrap();

        assert_eq!(result.file_count, 1);

        let commit = repo.find_commit(result.commit_id).unwrap();
        let tree = commit.tree().unwrap();
        assert!(tree.get_name("live.md").is_some());
        assert!(tree.get_name("deleted.md").is_none());
    }

    #[test]
    fn test_multi_commit() {
        let (storage, workspace, body_docs, _dir, repo) = setup();

        // First commit
        let meta = FileMetadata::with_filename("file.md".to_string(), Some("File".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();
        let body_key = format!("body:test-ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Version 1")
            .unwrap();

        let mut tracker = HealthTracker::new();
        let options = CommitOptions {
            message: Some("First commit".to_string()),
            ..CommitOptions::default()
        };
        let result1 = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &options,
            &mut tracker,
        )
        .unwrap();

        // Second commit with changed content
        body_docs
            .get_or_create(&body_key)
            .set_body("Version 2")
            .unwrap();

        let options2 = CommitOptions {
            message: Some("Second commit".to_string()),
            ..CommitOptions::default()
        };
        let result2 = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "test-ws",
            &options2,
            &mut tracker,
        )
        .unwrap();

        assert_ne!(result1.commit_id, result2.commit_id);

        // Verify git log has two commits
        let mut revwalk = repo.revwalk().unwrap();
        revwalk.push_head().unwrap();
        let commits: Vec<_> = revwalk.collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(commits.len(), 2);
    }
}
