//! Permission checker implementations for Extism host function gating.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use diaryx_core::frontmatter;
use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::permissions::{
    PermissionCheck, PermissionType, PluginConfig, check_permission,
};
use diaryx_core::workspace::Workspace;

use crate::host_fns::PermissionChecker;

/// A strict checker that denies every permission request.
pub struct DenyAllPermissionChecker;

impl PermissionChecker for DenyAllPermissionChecker {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String> {
        Err(format!(
            "Permission denied: no permissions configured for plugin '{}' ({} on '{}')",
            plugin_id,
            permission_type.key(),
            target
        ))
    }
}

/// Loads plugin permissions from root frontmatter `plugins` on each check.
pub struct FrontmatterPermissionChecker {
    root_index_path: Option<PathBuf>,
}

impl FrontmatterPermissionChecker {
    /// Build a checker from a workspace directory path.
    pub fn from_workspace_root(workspace_root: Option<PathBuf>) -> Self {
        let root_index_path = workspace_root.as_deref().and_then(find_root_index_path);
        Self { root_index_path }
    }

    fn load_plugins_config(&self) -> Result<HashMap<String, PluginConfig>, String> {
        let root_path = self.root_index_path.as_ref().ok_or_else(|| {
            "Workspace root index not available for permission checks".to_string()
        })?;

        let content = std::fs::read_to_string(root_path)
            .map_err(|e| format!("Failed to read root index '{}': {e}", root_path.display()))?;
        let parsed = frontmatter::parse_or_empty(&content)
            .map_err(|e| format!("Failed to parse root frontmatter: {e}"))?;

        let plugins_value = match parsed.frontmatter.get("plugins") {
            Some(v) => v.clone(),
            None => return Ok(HashMap::new()),
        };

        serde_yaml::from_value::<HashMap<String, PluginConfig>>(plugins_value)
            .map_err(|e| format!("Invalid root frontmatter plugins config: {e}"))
    }
}

impl PermissionChecker for FrontmatterPermissionChecker {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String> {
        let plugins_config = self.load_plugins_config()?;
        match check_permission(&plugins_config, plugin_id, permission_type, target) {
            PermissionCheck::Allowed => Ok(()),
            PermissionCheck::Denied => Err(format!(
                "Permission denied for plugin '{}': {} on '{}'",
                plugin_id,
                permission_type.key(),
                target
            )),
            PermissionCheck::NotConfigured => Err(format!(
                "Permission not configured for plugin '{}': {} on '{}'. \
                 Add plugins.{}.permissions.{} in root frontmatter.",
                plugin_id,
                permission_type.key(),
                target,
                plugin_id,
                permission_type.key(),
            )),
        }
    }
}

fn find_root_index_path(workspace_root: &Path) -> Option<PathBuf> {
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let workspace = Workspace::new(fs);
    futures_lite::future::block_on(workspace.find_root_index_in_dir(workspace_root))
        .ok()
        .flatten()
}
