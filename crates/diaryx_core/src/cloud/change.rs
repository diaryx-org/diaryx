//! Change detection types for sync operations.
//!
//! This module defines the types used to represent local and remote changes,
//! and the actions that need to be taken during sync.

use super::RemoteFileInfo;
use super::conflict::ConflictInfo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A change detected in the local workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LocalChange {
    /// A new file was created locally
    Created {
        /// Path to the file
        path: String,
        /// Content hash
        content_hash: String,
        /// Modification timestamp
        modified_at: i64,
    },
    /// An existing file was modified locally
    Modified {
        /// Path to the file
        path: String,
        /// New content hash
        content_hash: String,
        /// Modification timestamp
        modified_at: i64,
        /// Previous content hash from manifest
        previous_hash: String,
    },
    /// A file was deleted locally
    Deleted {
        /// Path to the deleted file
        path: String,
    },
}

impl LocalChange {
    /// Get the path of the changed file
    pub fn path(&self) -> &str {
        match self {
            LocalChange::Created { path, .. } => path,
            LocalChange::Modified { path, .. } => path,
            LocalChange::Deleted { path } => path,
        }
    }

    /// Get the content hash if available
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            LocalChange::Created { content_hash, .. } => Some(content_hash),
            LocalChange::Modified { content_hash, .. } => Some(content_hash),
            LocalChange::Deleted { .. } => None,
        }
    }

    /// Get the modification timestamp if available
    pub fn modified_at(&self) -> Option<i64> {
        match self {
            LocalChange::Created { modified_at, .. } => Some(*modified_at),
            LocalChange::Modified { modified_at, .. } => Some(*modified_at),
            LocalChange::Deleted { .. } => None,
        }
    }
}

/// A change detected in remote storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RemoteChange {
    /// A new file was created remotely
    Created {
        /// Remote file information
        info: RemoteFileInfo,
    },
    /// An existing file was modified remotely
    Modified {
        /// Updated remote file information
        info: RemoteFileInfo,
        /// Previous version identifier
        previous_version: Option<String>,
    },
    /// A file was deleted remotely
    Deleted {
        /// Path to the deleted file
        path: String,
    },
}

impl RemoteChange {
    /// Get the path of the changed file
    pub fn path(&self) -> &str {
        match self {
            RemoteChange::Created { info } => &info.path,
            RemoteChange::Modified { info, .. } => &info.path,
            RemoteChange::Deleted { path } => path,
        }
    }

    /// Get the modification timestamp if available
    pub fn modified_at(&self) -> Option<DateTime<Utc>> {
        match self {
            RemoteChange::Created { info } => Some(info.modified_at),
            RemoteChange::Modified { info, .. } => Some(info.modified_at),
            RemoteChange::Deleted { .. } => None,
        }
    }

    /// Get the content hash if available
    pub fn content_hash(&self) -> Option<&str> {
        match self {
            RemoteChange::Created { info } => info.content_hash.as_deref(),
            RemoteChange::Modified { info, .. } => info.content_hash.as_deref(),
            RemoteChange::Deleted { .. } => None,
        }
    }
}

/// Direction of sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncDirection {
    /// From local to remote (upload)
    Upload,
    /// From remote to local (download)
    Download,
}

/// An action to be taken during sync.
#[derive(Debug, Clone)]
pub enum SyncAction {
    /// Upload a file to remote storage
    Upload {
        /// Path to upload
        path: String,
    },
    /// Download a file from remote storage
    Download {
        /// Path to download
        path: String,
        /// Remote file info
        remote_info: RemoteFileInfo,
    },
    /// Delete a file
    Delete {
        /// Path to delete
        path: String,
        /// Where to delete from
        direction: SyncDirection,
    },
    /// A conflict that needs resolution
    Conflict {
        /// Conflict information
        info: ConflictInfo,
    },
    /// Clean up a manifest entry without any file operation.
    ///
    /// Used when both local and remote have deleted the same file -
    /// neither side has it, so we just remove the stale manifest entry.
    ManifestCleanup {
        /// Path to remove from manifest
        path: String,
    },
}

impl SyncAction {
    /// Get the path this action affects
    pub fn path(&self) -> &str {
        match self {
            SyncAction::Upload { path } => path,
            SyncAction::Download { path, .. } => path,
            SyncAction::Delete { path, .. } => path,
            SyncAction::Conflict { info } => &info.path,
            SyncAction::ManifestCleanup { path } => path,
        }
    }

    /// Check if this is an upload action
    pub fn is_upload(&self) -> bool {
        matches!(self, SyncAction::Upload { .. })
    }

    /// Check if this is a download action
    pub fn is_download(&self) -> bool {
        matches!(self, SyncAction::Download { .. })
    }

    /// Check if this is a conflict
    pub fn is_conflict(&self) -> bool {
        matches!(self, SyncAction::Conflict { .. })
    }
}

/// Detect conflicts between local and remote changes.
///
/// A conflict occurs when the same file was modified on both sides since the last sync.
pub fn detect_conflicts(
    local_changes: &[LocalChange],
    remote_changes: &[RemoteChange],
) -> Vec<ConflictInfo> {
    let mut conflicts = Vec::new();

    for local in local_changes {
        // Skip deletions for conflict detection
        if matches!(local, LocalChange::Deleted { .. }) {
            continue;
        }

        for remote in remote_changes {
            // Skip deletions for conflict detection
            if matches!(remote, RemoteChange::Deleted { .. }) {
                continue;
            }

            if local.path() == remote.path() {
                // Both sides modified the same file
                conflicts.push(ConflictInfo {
                    path: local.path().to_string(),
                    local_modified_at: local.modified_at(),
                    remote_modified_at: remote.modified_at(),
                    local_hash: local.content_hash().map(String::from),
                    remote_hash: remote.content_hash().map(String::from),
                });
            }
        }
    }

    conflicts
}

/// Compute sync actions from local and remote changes.
///
/// This function determines what operations need to be performed to sync,
/// handling conflicts appropriately. It also handles cross-deletion scenarios:
///
/// - **Local delete + remote modify**: Local deletion takes precedence. The file
///   is deleted from remote (no download).
/// - **Local modify + remote delete**: Local modification takes precedence. The
///   file is uploaded to remote (no local delete).
/// - **Both sides deleted**: No action needed; manifest cleanup only.
pub fn compute_sync_actions(
    local_changes: &[LocalChange],
    remote_changes: &[RemoteChange],
) -> Vec<SyncAction> {
    let conflicts = detect_conflicts(local_changes, remote_changes);
    let conflict_paths: std::collections::HashSet<String> =
        conflicts.iter().map(|c| c.path.clone()).collect();

    // Build sets for cross-deletion handling
    let locally_deleted: std::collections::HashSet<String> = local_changes
        .iter()
        .filter_map(|c| match c {
            LocalChange::Deleted { path } => Some(path.clone()),
            _ => None,
        })
        .collect();

    let remotely_deleted: std::collections::HashSet<String> = remote_changes
        .iter()
        .filter_map(|c| match c {
            RemoteChange::Deleted { path } => Some(path.clone()),
            _ => None,
        })
        .collect();

    let mut actions = Vec::new();

    // Add conflict actions first
    for conflict in conflicts {
        actions.push(SyncAction::Conflict { info: conflict });
    }

    // Process local changes (excluding conflicts)
    for change in local_changes {
        if conflict_paths.contains(change.path()) {
            continue;
        }

        match change {
            LocalChange::Created { path, .. } | LocalChange::Modified { path, .. } => {
                // Local create/modify always generates an upload.
                // If the file was remotely deleted, we still upload to preserve
                // local modifications (local wins).
                actions.push(SyncAction::Upload { path: path.clone() });
            }
            LocalChange::Deleted { path } => {
                if remotely_deleted.contains(path) {
                    // Both sides deleted - no action needed.
                    // Emit a ManifestCleanup action so the manifest entry is removed.
                    actions.push(SyncAction::ManifestCleanup {
                        path: path.clone(),
                    });
                } else {
                    // Normal local deletion or local delete + remote modify:
                    // delete from remote (local deletion takes precedence).
                    actions.push(SyncAction::Delete {
                        path: path.clone(),
                        direction: SyncDirection::Upload,
                    });
                }
            }
        }
    }

    // Process remote changes (excluding conflicts and cross-deletion paths)
    for change in remote_changes {
        if conflict_paths.contains(change.path()) {
            continue;
        }

        match change {
            RemoteChange::Created { info } | RemoteChange::Modified { info, .. } => {
                if locally_deleted.contains(&info.path) {
                    // File was modified/created remotely but deleted locally.
                    // Local deletion takes precedence - skip download.
                    continue;
                }
                actions.push(SyncAction::Download {
                    path: info.path.clone(),
                    remote_info: info.clone(),
                });
            }
            RemoteChange::Deleted { path } => {
                if locally_deleted.contains(path) {
                    // Both sides deleted - already handled above as ManifestCleanup.
                    continue;
                }
                // Check if the file was locally modified/created - if so, skip
                // the remote delete (local modification takes precedence).
                let locally_changed = local_changes.iter().any(|c| match c {
                    LocalChange::Created { path: p, .. }
                    | LocalChange::Modified { path: p, .. } => p == path,
                    _ => false,
                });
                if locally_changed {
                    continue;
                }
                actions.push(SyncAction::Delete {
                    path: path.clone(),
                    direction: SyncDirection::Download,
                });
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_remote_info(path: &str) -> RemoteFileInfo {
        RemoteFileInfo {
            path: path.to_string(),
            size: 100,
            modified_at: Utc::now(),
            etag: None,
            content_hash: Some("remote_hash".to_string()),
        }
    }

    #[test]
    fn test_detect_no_conflicts() {
        let local = vec![LocalChange::Created {
            path: "a.md".to_string(),
            content_hash: "hash_a".to_string(),
            modified_at: 1000,
        }];
        let remote = vec![RemoteChange::Created {
            info: make_remote_info("b.md"),
        }];

        let conflicts = detect_conflicts(&local, &remote);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_detect_conflict() {
        let local = vec![LocalChange::Modified {
            path: "shared.md".to_string(),
            content_hash: "local_hash".to_string(),
            modified_at: 2000,
            previous_hash: "old_hash".to_string(),
        }];
        let remote = vec![RemoteChange::Modified {
            info: make_remote_info("shared.md"),
            previous_version: None,
        }];

        let conflicts = detect_conflicts(&local, &remote);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "shared.md");
    }

    #[test]
    fn test_compute_sync_actions() {
        let local = vec![
            LocalChange::Created {
                path: "new_local.md".to_string(),
                content_hash: "h1".to_string(),
                modified_at: 1000,
            },
            LocalChange::Deleted {
                path: "deleted_local.md".to_string(),
            },
        ];
        let remote = vec![RemoteChange::Created {
            info: make_remote_info("new_remote.md"),
        }];

        let actions = compute_sync_actions(&local, &remote);

        assert_eq!(actions.len(), 3);

        // Should have: 1 upload, 1 delete (remote), 1 download
        let uploads: Vec<_> = actions.iter().filter(|a| a.is_upload()).collect();
        let downloads: Vec<_> = actions.iter().filter(|a| a.is_download()).collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();

        assert_eq!(uploads.len(), 1);
        assert_eq!(downloads.len(), 1);
        assert_eq!(deletes.len(), 1);
    }

    #[test]
    fn test_conflict_excludes_from_actions() {
        let local = vec![LocalChange::Modified {
            path: "conflict.md".to_string(),
            content_hash: "local".to_string(),
            modified_at: 2000,
            previous_hash: "old".to_string(),
        }];
        let remote = vec![RemoteChange::Modified {
            info: make_remote_info("conflict.md"),
            previous_version: None,
        }];

        let actions = compute_sync_actions(&local, &remote);

        // Should only have conflict action, no upload/download for the conflicting file
        assert_eq!(actions.len(), 1);
        assert!(actions[0].is_conflict());
    }

    #[test]
    fn test_local_delete_suppresses_remote_download() {
        // File deleted locally but modified remotely - local delete should win,
        // no download should be generated (this was causing resurrected files).
        let local = vec![LocalChange::Deleted {
            path: "deleted.md".to_string(),
        }];
        let remote = vec![RemoteChange::Modified {
            info: make_remote_info("deleted.md"),
            previous_version: Some("old-etag".to_string()),
        }];

        let actions = compute_sync_actions(&local, &remote);

        // Should only have a delete-from-remote action, NO download
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();
        let downloads: Vec<_> = actions.iter().filter(|a| a.is_download()).collect();

        assert_eq!(deletes.len(), 1, "Should have exactly one delete action");
        assert_eq!(downloads.len(), 0, "Should NOT download a locally-deleted file");

        if let SyncAction::Delete { direction, .. } = &deletes[0] {
            assert_eq!(*direction, SyncDirection::Upload, "Should delete from remote");
        }
    }

    #[test]
    fn test_local_modify_suppresses_remote_delete() {
        // File modified locally but deleted remotely - local modification should win,
        // no local delete should be generated.
        let local = vec![LocalChange::Modified {
            path: "modified.md".to_string(),
            content_hash: "new_hash".to_string(),
            modified_at: 2000,
            previous_hash: "old_hash".to_string(),
        }];
        let remote = vec![RemoteChange::Deleted {
            path: "modified.md".to_string(),
        }];

        let actions = compute_sync_actions(&local, &remote);

        // Should only have an upload action, NO local delete
        let uploads: Vec<_> = actions.iter().filter(|a| a.is_upload()).collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();

        assert_eq!(uploads.len(), 1, "Should upload the locally modified file");
        assert_eq!(deletes.len(), 0, "Should NOT delete a locally-modified file");
    }

    #[test]
    fn test_both_sides_deleted_produces_manifest_cleanup() {
        // File deleted on both local and remote - should produce a ManifestCleanup,
        // not conflicting delete actions that would fail.
        let local = vec![LocalChange::Deleted {
            path: "gone.md".to_string(),
        }];
        let remote = vec![RemoteChange::Deleted {
            path: "gone.md".to_string(),
        }];

        let actions = compute_sync_actions(&local, &remote);

        // Should only have a ManifestCleanup, no Delete actions
        let cleanups: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::ManifestCleanup { .. }))
            .collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .collect();

        assert_eq!(cleanups.len(), 1, "Should have one manifest cleanup");
        assert_eq!(deletes.len(), 0, "Should have no delete actions");
    }

    #[test]
    fn test_mixed_changes_with_cross_deletions() {
        // Complex scenario: multiple files with various change combinations
        let local = vec![
            LocalChange::Created {
                path: "new_local.md".to_string(),
                content_hash: "h1".to_string(),
                modified_at: 1000,
            },
            LocalChange::Deleted {
                path: "deleted_both_sides.md".to_string(),
            },
            LocalChange::Deleted {
                path: "deleted_local_modified_remote.md".to_string(),
            },
            LocalChange::Modified {
                path: "modified_local_deleted_remote.md".to_string(),
                content_hash: "new".to_string(),
                modified_at: 2000,
                previous_hash: "old".to_string(),
            },
        ];
        let remote = vec![
            RemoteChange::Created {
                info: make_remote_info("new_remote.md"),
            },
            RemoteChange::Deleted {
                path: "deleted_both_sides.md".to_string(),
            },
            RemoteChange::Modified {
                info: make_remote_info("deleted_local_modified_remote.md"),
                previous_version: None,
            },
            RemoteChange::Deleted {
                path: "modified_local_deleted_remote.md".to_string(),
            },
        ];

        let actions = compute_sync_actions(&local, &remote);

        // Expected actions:
        // 1. Upload "new_local.md" (new local file)
        // 2. ManifestCleanup "deleted_both_sides.md" (both deleted)
        // 3. Delete "deleted_local_modified_remote.md" from remote (local delete wins)
        // 4. Upload "modified_local_deleted_remote.md" (local modify wins)
        // 5. Download "new_remote.md" (new remote file)
        let uploads: Vec<_> = actions
            .iter()
            .filter(|a| a.is_upload())
            .map(|a| a.path())
            .collect();
        let downloads: Vec<_> = actions
            .iter()
            .filter(|a| a.is_download())
            .map(|a| a.path())
            .collect();
        let deletes: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::Delete { .. }))
            .map(|a| a.path())
            .collect();
        let cleanups: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, SyncAction::ManifestCleanup { .. }))
            .map(|a| a.path())
            .collect();

        assert_eq!(uploads.len(), 2);
        assert!(uploads.contains(&"new_local.md"));
        assert!(uploads.contains(&"modified_local_deleted_remote.md"));

        assert_eq!(downloads.len(), 1);
        assert!(downloads.contains(&"new_remote.md"));

        assert_eq!(deletes.len(), 1);
        assert!(deletes.contains(&"deleted_local_modified_remote.md"));

        assert_eq!(cleanups.len(), 1);
        assert!(cleanups.contains(&"deleted_both_sides.md"));
    }
}
