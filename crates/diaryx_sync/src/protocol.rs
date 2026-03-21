//! Server-specific sync protocol extensions.
//!
//! Re-exports all types from [`protocol_types`] and adds server-only types
//! that depend on tokio (e.g., [`DirtyWorkspaces`]).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Re-export everything from protocol_types for backward compatibility
pub use crate::protocol_types::*;

/// Tracks when workspaces were last modified (for git auto-commit quiescence detection).
pub type DirtyWorkspaces = Arc<RwLock<HashMap<String, tokio::time::Instant>>>;
