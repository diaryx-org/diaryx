//! Workspace-scoped config persistence via `host::storage`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use diaryx_plugin_sdk::prelude::*;

use crate::daily_logic::DailyPluginConfig;
use crate::state::DailyState;

pub fn storage_key_for_workspace(workspace_root: Option<&str>) -> String {
    let token = workspace_root.unwrap_or("__default__");
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    format!("daily.config.{:x}", hasher.finish())
}

pub fn load_workspace_config(workspace_root: Option<&str>) -> DailyPluginConfig {
    let key = storage_key_for_workspace(workspace_root);
    match host::storage::get(&key) {
        Ok(Some(bytes)) => serde_json::from_slice::<DailyPluginConfig>(&bytes).unwrap_or_default(),
        _ => DailyPluginConfig::default(),
    }
}

pub fn save_workspace_config(state: &DailyState) -> Result<(), String> {
    let key = storage_key_for_workspace(state.workspace_root.as_deref());
    let bytes = serde_json::to_vec(&state.config).map_err(|e| format!("serialize config: {e}"))?;
    host::storage::set(&key, &bytes)?;
    Ok(())
}
