//! Workspace-scoped config persistence via `host::storage`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use diaryx_plugin_sdk::prelude::*;

use crate::daily_logic::DailyPluginConfig;

pub fn storage_key_for_workspace(workspace_root: Option<&str>) -> String {
    let token = workspace_root.unwrap_or("__default__");
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    format!("daily.config.{:x}", hasher.finish())
}

/// Read the config the plugin may still hold in `host::storage` (the legacy
/// store). Read-only: this is the migration source. Declarative config is now
/// persisted by the host to `plugins.diaryx.daily.config`, not here.
pub fn load_workspace_config(workspace_root: Option<&str>) -> DailyPluginConfig {
    let key = storage_key_for_workspace(workspace_root);
    match host::storage::get(&key) {
        Ok(Some(bytes)) => serde_json::from_slice::<DailyPluginConfig>(&bytes).unwrap_or_default(),
        _ => DailyPluginConfig::default(),
    }
}
