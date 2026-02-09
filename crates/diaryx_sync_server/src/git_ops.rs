//! Git operations for server-side workspace commits and restores.
//!
//! Thin wrappers around `diaryx_core::crdt::git` that handle storage lookup,
//! repo open/init, and CRDT loading for a given workspace ID.

use std::sync::Arc;

use diaryx_core::crdt::git::{
    CommitOptions, CommitResult, RepoKind, commit_workspace, init_repo, open_repo,
    rebuild_crdt_from_git,
};
use diaryx_core::crdt::self_healing::HealthTracker;
use diaryx_core::crdt::{BodyDocManager, CrdtStorage, WorkspaceCrdt};
use diaryx_core::error::DiaryxError;
use git2::Oid;

use crate::sync_v2::StorageCache;

/// Commit the current workspace state to its bare git repo.
///
/// Opens (or creates) a bare repo at `{workspaces_dir}/{workspace_id}.git`,
/// materializes the CRDT state, and creates a git commit.
pub fn commit_workspace_by_id(
    storage_cache: &Arc<StorageCache>,
    workspace_id: &str,
    message: Option<String>,
) -> Result<CommitResult, DiaryxError> {
    let storage = storage_cache
        .get_storage(workspace_id)
        .map_err(DiaryxError::Git)?;

    let workspace_doc_name = format!("workspace:{}", workspace_id);
    let workspace = WorkspaceCrdt::load_with_name(storage.clone(), workspace_doc_name)?;
    let body_docs = BodyDocManager::new(storage.clone());

    let repo_path = storage_cache.git_repo_path(workspace_id);
    let repo = match open_repo(&repo_path) {
        Ok(r) => r,
        Err(_) => init_repo(&repo_path, RepoKind::Bare)?,
    };

    let options = CommitOptions {
        message,
        skip_validation: false,
        ..CommitOptions::default()
    };

    let mut tracker = HealthTracker::new();
    commit_workspace(
        &(storage as Arc<dyn CrdtStorage>),
        &workspace,
        &body_docs,
        &repo,
        workspace_id,
        &options,
        &mut tracker,
    )
}

/// Restore a workspace's CRDT state from a specific git commit.
///
/// This is a destructive operation — existing CRDT state is replaced.
pub fn restore_workspace_by_id(
    storage_cache: &Arc<StorageCache>,
    workspace_id: &str,
    commit_id: Oid,
) -> Result<usize, DiaryxError> {
    let storage = storage_cache
        .get_storage(workspace_id)
        .map_err(DiaryxError::Git)?;

    let repo_path = storage_cache.git_repo_path(workspace_id);
    let repo = open_repo(&repo_path)?;

    rebuild_crdt_from_git(
        &repo,
        &(storage as Arc<dyn CrdtStorage>),
        workspace_id,
        Some(commit_id),
    )
}
