//! Workspace-scoped configuration helpers.
//!
//! Many plugins store per-workspace configuration in plugin storage, keyed
//! by a hash of the workspace root path. This module provides helpers
//! for that pattern.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Generate a storage key scoped to a specific workspace.
///
/// Uses a hash of the workspace root path to namespace config keys.
///
/// # Example
///
/// ```rust,ignore
/// use diaryx_plugin_sdk::config;
///
/// let key = config::workspace_key("myplugin.config", Some("/Users/me/diary"));
/// // Returns something like "myplugin.config.a1b2c3d4e5f6"
/// ```
pub fn workspace_key(prefix: &str, workspace_root: Option<&str>) -> String {
    let mut hasher = DefaultHasher::new();
    workspace_root.unwrap_or("__default__").hash(&mut hasher);
    format!("{}.{:x}", prefix, hasher.finish())
}

/// Load workspace-scoped configuration from storage.
///
/// Returns `None` if no configuration exists for this workspace.
#[cfg(feature = "core")]
pub fn load<T: serde::de::DeserializeOwned>(
    prefix: &str,
    workspace_root: Option<&str>,
) -> Result<Option<T>, String> {
    let key = workspace_key(prefix, workspace_root);
    crate::host::storage::get_json(&key)
}

/// Save workspace-scoped configuration to storage.
#[cfg(feature = "core")]
pub fn save<T: serde::Serialize>(
    prefix: &str,
    workspace_root: Option<&str>,
    value: &T,
) -> Result<(), String> {
    let key = workspace_key(prefix, workspace_root);
    crate::host::storage::set_json(&key, value)
}
