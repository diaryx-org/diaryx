//! Git-backed version history for workspace CRDTs.
//!
//! This module provides commit, compact, and rebuild operations that use git
//! as the authoritative history store. After each commit, CRDT update logs
//! can be compacted since the workspace state is now captured in git.
//!
//! # Feature Gate
//!
//! This module requires the `git` feature and is only available on native
//! platforms (not WASM).

mod commit;
mod rebuild;
mod repo;

pub use commit::{CommitOptions, CommitResult, commit_workspace, compact_workspace};
pub use rebuild::rebuild_crdt_from_git;
pub use repo::{RepoKind, init_repo, open_repo};
