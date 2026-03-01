//! Git commit workflow — take materialized files, validate, commit.

use git2::{Oid, Repository, Signature};
use serde::{Deserialize, Serialize};

use crate::self_healing::{HealingAction, HealthTracker};
use diaryx_core::error::DiaryxError;

/// A single materialized file ready for git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedFile {
    /// Workspace-relative path (e.g. `Daily/2024/note.md`).
    pub path: String,
    /// Full file content (frontmatter + body).
    pub content: String,
}

/// Options for a workspace commit.
pub struct CommitOptions {
    /// Commit message. Defaults to an auto-generated timestamp message.
    pub message: Option<String>,
    /// Author name for the commit.
    pub author_name: String,
    /// Author email for the commit.
    pub author_email: String,
    /// Whether to skip validation before committing.
    pub skip_validation: bool,
}

impl Default for CommitOptions {
    fn default() -> Self {
        Self {
            message: None,
            author_name: "Diaryx".to_string(),
            author_email: "noreply@diaryx.app".to_string(),
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
}

/// Build a git commit from pre-materialized files.
///
/// # Workflow
/// 1. Validate file count (unless skipped)
/// 2. Build git tree in-memory → handles nested dirs
/// 3. Create commit → pointing to tree
///
/// # Arguments
///
/// * `files` — Pre-materialized files from the sync plugin's `MaterializeWorkspace` command.
/// * `repo` — Git repository to commit into.
/// * `options` — Commit configuration.
/// * `health_tracker` — Tracks validation failures for self-healing.
pub fn commit_workspace(
    files: &[MaterializedFile],
    repo: &Repository,
    options: &CommitOptions,
    health_tracker: &mut HealthTracker,
) -> Result<CommitResult, DiaryxError> {
    if files.is_empty() {
        return Err(DiaryxError::Git("No files to commit".to_string()));
    }

    // Step 1: Validate (basic check — file count)
    if !options.skip_validation {
        // Simple validation: check for files with empty paths or empty content
        let issues: Vec<String> = files
            .iter()
            .filter(|f| f.path.is_empty() || f.content.is_empty())
            .map(|f| {
                if f.path.is_empty() {
                    "File with empty path".to_string()
                } else {
                    format!("File '{}' has empty content", f.path)
                }
            })
            .collect();

        if !issues.is_empty() {
            let action = health_tracker.record_failure();
            let issue_summary: Vec<String> = issues.into_iter().take(5).collect();
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

    // Step 2: Build git tree
    let tree_oid = build_tree(repo, files)?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| DiaryxError::Git(e.to_string()))?;

    // Step 3: Create commit
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

    let file_count = files.len();

    Ok(CommitResult {
        commit_id: commit_oid,
        file_count,
    })
}

/// Build a git tree from materialized files, handling nested directories.
fn build_tree(repo: &Repository, files: &[MaterializedFile]) -> Result<Oid, DiaryxError> {
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
    use crate::repo::{RepoKind, init_repo};

    #[test]
    fn test_commit_single_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        let files = vec![MaterializedFile {
            path: "hello.md".to_string(),
            content: "---\ntitle: Hello\n---\n\nHello world".to_string(),
        }];

        let mut tracker = HealthTracker::new();
        let result =
            commit_workspace(&files, &repo, &CommitOptions::default(), &mut tracker).unwrap();

        assert_eq!(result.file_count, 1);

        let commit = repo.find_commit(result.commit_id).unwrap();
        let tree = commit.tree().unwrap();
        assert!(tree.get_name("hello.md").is_some());
    }

    #[test]
    fn test_commit_nested_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        let files = vec![
            MaterializedFile {
                path: "daily/2024-01-01.md".to_string(),
                content: "---\ntitle: Jan 1\n---\n\nNew year content".to_string(),
            },
            MaterializedFile {
                path: "daily/README.md".to_string(),
                content: "---\ntitle: Daily\n---\n\nDaily index".to_string(),
            },
        ];

        let mut tracker = HealthTracker::new();
        let result =
            commit_workspace(&files, &repo, &CommitOptions::default(), &mut tracker).unwrap();

        let commit = repo.find_commit(result.commit_id).unwrap();
        let tree = commit.tree().unwrap();
        let daily_entry = tree.get_name("daily").unwrap();
        let daily_tree = repo.find_tree(daily_entry.id()).unwrap();
        assert!(daily_tree.get_name("2024-01-01.md").is_some());
        assert!(daily_tree.get_name("README.md").is_some());
    }

    #[test]
    fn test_commit_empty_workspace_fails() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        let mut tracker = HealthTracker::new();
        let result = commit_workspace(&[], &repo, &CommitOptions::default(), &mut tracker);

        assert!(result.is_err());
    }

    #[test]
    fn test_multi_commit() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo(dir.path(), RepoKind::Standard).unwrap();

        let files1 = vec![MaterializedFile {
            path: "file.md".to_string(),
            content: "Version 1".to_string(),
        }];

        let mut tracker = HealthTracker::new();
        let options1 = CommitOptions {
            message: Some("First commit".to_string()),
            ..CommitOptions::default()
        };
        let result1 = commit_workspace(&files1, &repo, &options1, &mut tracker).unwrap();

        let files2 = vec![MaterializedFile {
            path: "file.md".to_string(),
            content: "Version 2".to_string(),
        }];

        let options2 = CommitOptions {
            message: Some("Second commit".to_string()),
            ..CommitOptions::default()
        };
        let result2 = commit_workspace(&files2, &repo, &options2, &mut tracker).unwrap();

        assert_ne!(result1.commit_id, result2.commit_id);

        let mut revwalk = repo.revwalk().unwrap();
        revwalk.push_head().unwrap();
        let commits: Vec<_> = revwalk.collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(commits.len(), 2);
    }
}
