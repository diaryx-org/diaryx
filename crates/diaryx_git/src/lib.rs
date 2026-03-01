//! Git version history for Diaryx workspaces.
//!
//! This crate provides git-based snapshotting of workspace state.
//! It takes materialized file data as input (from the sync plugin),
//! so it has no dependency on `diaryx_sync`.

pub mod commit;
mod repo;
mod self_healing;

pub use commit::{CommitOptions, CommitResult, MaterializedFile, commit_workspace};
pub use repo::{RepoKind, init_repo, open_repo};
pub use self_healing::{HealingAction, HealthTracker};
