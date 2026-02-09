//! CRDT rebuild from git — self-healing by replaying git state into CRDT.

use std::sync::Arc;

use git2::{Oid, Repository};

use crate::crdt::body_doc_manager::BodyDocManager;
use crate::crdt::materialize::parse_snapshot_markdown;
use crate::crdt::storage::CrdtStorage;
use crate::crdt::workspace_doc::WorkspaceCrdt;
use crate::error::DiaryxError;

/// Rebuild CRDT state from a git commit.
///
/// This clears the existing CRDT state and re-populates it by walking the
/// git tree at the specified commit (or HEAD if `commit_id` is None).
///
/// # Warning
///
/// This is a destructive operation — all existing CRDT history is lost.
/// Only use this when the CRDT is known to be in a bad state and git
/// has the authoritative data.
///
/// # Arguments
///
/// * `repo` — Git repository to read from.
/// * `storage` — CRDT storage to rebuild into.
/// * `workspace_id` — Workspace identifier.
/// * `commit_id` — Specific commit to rebuild from, or None for HEAD.
pub fn rebuild_crdt_from_git(
    repo: &Repository,
    storage: &Arc<dyn CrdtStorage>,
    workspace_id: &str,
    commit_id: Option<Oid>,
) -> Result<usize, DiaryxError> {
    // Resolve commit
    let commit = match commit_id {
        Some(oid) => repo
            .find_commit(oid)
            .map_err(|e| DiaryxError::Git(format!("Commit not found: {}", e)))?,
        None => {
            let head = repo
                .head()
                .map_err(|e| DiaryxError::Git(format!("No HEAD: {}", e)))?;
            head.peel_to_commit()
                .map_err(|e| DiaryxError::Git(format!("HEAD is not a commit: {}", e)))?
        }
    };

    let tree = commit.tree().map_err(|e| DiaryxError::Git(e.to_string()))?;

    // Clear existing CRDT state
    let existing_docs = storage.list_docs()?;
    let workspace_prefix = format!("workspace:{}", workspace_id);
    let body_prefix = format!("body:{}/", workspace_id);

    for doc_name in &existing_docs {
        if doc_name == &workspace_prefix || doc_name.starts_with(&body_prefix) {
            storage.delete_doc(doc_name)?;
        }
    }

    // Create fresh workspace and body doc manager
    let workspace_doc_name = format!("workspace:{}", workspace_id);
    let workspace = WorkspaceCrdt::with_name(storage.clone(), workspace_doc_name);
    let body_docs = BodyDocManager::new(storage.clone());

    // Walk tree and rebuild
    let mut file_count = 0;
    walk_tree(
        repo,
        &tree,
        "",
        &workspace,
        &body_docs,
        workspace_id,
        &mut file_count,
    )?;

    Ok(file_count)
}

/// Recursively walk a git tree and populate CRDT state.
fn walk_tree(
    repo: &Repository,
    tree: &git2::Tree,
    prefix: &str,
    workspace: &WorkspaceCrdt,
    body_docs: &BodyDocManager,
    workspace_id: &str,
    file_count: &mut usize,
) -> Result<(), DiaryxError> {
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("");
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", prefix, name)
        };

        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                if !path.ends_with(".md") {
                    continue;
                }

                let blob = repo
                    .find_blob(entry.id())
                    .map_err(|e| DiaryxError::Git(e.to_string()))?;
                let content = std::str::from_utf8(blob.content()).map_err(|e| {
                    DiaryxError::Git(format!("Non-UTF8 content in {}: {}", path, e))
                })?;

                let (metadata, body) = parse_snapshot_markdown(&path, content)
                    .map_err(|e| DiaryxError::Git(format!("Parse error in {}: {}", path, e)))?;

                workspace.set_file(&path, metadata)?;

                // Trim leading newline that parse_or_empty adds after frontmatter delimiter.
                let body_trimmed = body.strip_prefix('\n').unwrap_or(&body);
                let body_key = format!("body:{}/{}", workspace_id, path);
                body_docs.get_or_create(&body_key).set_body(body_trimmed)?;

                *file_count += 1;
            }
            Some(git2::ObjectType::Tree) => {
                let subtree = repo
                    .find_tree(entry.id())
                    .map_err(|e| DiaryxError::Git(e.to_string()))?;
                walk_tree(
                    repo,
                    &subtree,
                    &path,
                    workspace,
                    body_docs,
                    workspace_id,
                    file_count,
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::git::commit::{CommitOptions, commit_workspace};
    use crate::crdt::git::repo::{RepoKind, init_repo};
    use crate::crdt::self_healing::HealthTracker;
    use crate::crdt::{FileMetadata, MemoryStorage};

    #[test]
    fn test_rebuild_from_git() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage.clone());
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        // Create files and commit
        let meta = FileMetadata::with_filename("note.md".to_string(), Some("Note".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();
        let body_key = format!("body:ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Note content")
            .unwrap();

        let mut tracker = HealthTracker::new();
        let options = CommitOptions {
            skip_validation: true,
            ..CommitOptions::default()
        };
        commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "ws",
            &options,
            &mut tracker,
        )
        .unwrap();

        // Now use a fresh storage to rebuild into
        let new_storage = Arc::new(MemoryStorage::new());
        let count = rebuild_crdt_from_git(
            &repo,
            &(new_storage.clone() as Arc<dyn CrdtStorage>),
            "ws",
            None,
        )
        .unwrap();
        assert_eq!(count, 1);

        // Verify the rebuilt workspace has the file
        let ws_doc_name = "workspace:ws".to_string();
        let rebuilt_workspace =
            WorkspaceCrdt::load_with_name(new_storage.clone(), ws_doc_name).unwrap();
        let files = rebuilt_workspace.list_files();
        assert_eq!(files.len(), 1);

        let (_, rebuilt_meta) = &files[0];
        assert_eq!(rebuilt_meta.title, Some("Note".to_string()));

        // Verify body content
        let rebuilt_body_docs = BodyDocManager::new(new_storage);
        let rebuilt_body = rebuilt_body_docs.get_or_create("body:ws/note.md");
        assert_eq!(rebuilt_body.get_body(), "Note content");
    }

    #[test]
    fn test_rebuild_specific_commit() {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage.clone());
        let body_docs = BodyDocManager::new(storage.clone());
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        // First commit
        let meta = FileMetadata::with_filename("v1.md".to_string(), Some("V1".to_string()));
        let doc_id = workspace.create_file(meta).unwrap();
        let path = workspace.get_path(&doc_id).unwrap();
        let body_key = format!("body:ws/{}", path.to_string_lossy());
        body_docs
            .get_or_create(&body_key)
            .set_body("Version 1")
            .unwrap();

        let mut tracker = HealthTracker::new();
        let options = CommitOptions {
            message: Some("First".to_string()),
            skip_validation: true,
            ..CommitOptions::default()
        };
        let result1 = commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "ws",
            &options,
            &mut tracker,
        )
        .unwrap();

        // Second commit with more content
        let meta2 = FileMetadata::with_filename("v2.md".to_string(), Some("V2".to_string()));
        let doc_id2 = workspace.create_file(meta2).unwrap();
        let path2 = workspace.get_path(&doc_id2).unwrap();
        let body_key2 = format!("body:ws/{}", path2.to_string_lossy());
        body_docs
            .get_or_create(&body_key2)
            .set_body("Version 2")
            .unwrap();

        let options2 = CommitOptions {
            message: Some("Second".to_string()),
            skip_validation: true,
            ..CommitOptions::default()
        };
        commit_workspace(
            &(storage.clone() as Arc<dyn CrdtStorage>),
            &workspace,
            &body_docs,
            &repo,
            "ws",
            &options2,
            &mut tracker,
        )
        .unwrap();

        // Rebuild from first commit (should only have v1.md)
        let new_storage = Arc::new(MemoryStorage::new());
        let count = rebuild_crdt_from_git(
            &repo,
            &(new_storage.clone() as Arc<dyn CrdtStorage>),
            "ws",
            Some(result1.commit_id),
        )
        .unwrap();
        assert_eq!(count, 1);

        let ws_doc_name = "workspace:ws".to_string();
        let rebuilt = WorkspaceCrdt::load_with_name(new_storage, ws_doc_name).unwrap();
        let files = rebuilt.list_files();
        assert_eq!(files.len(), 1);
    }
}
