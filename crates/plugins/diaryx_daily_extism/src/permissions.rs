//! Permission scopes, manifest defaults, and patch generation.

use std::collections::HashMap;

use diaryx_plugin_sdk::prelude::*;
use serde_json::Value as JsonValue;

use crate::daily_logic::normalize_folder;
use crate::paths::{normalize_rel_path, root_index_scope};
use crate::state::with_state_mut;
use crate::storage::save_workspace_config;

pub fn unique_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for scope in scopes {
        if !scope.is_empty() && !out.iter().any(|existing| existing == &scope) {
            out.push(scope);
        }
    }
    out
}

pub fn requested_permissions_for(folder: &str, root_index_path: Option<&str>) -> JsonValue {
    let folder_scope = normalize_rel_path(folder);
    let root_scope = root_index_scope(root_index_path);
    let read_edit_scopes = unique_scopes(vec![folder_scope.clone(), root_scope]);

    serde_json::json!({
        "defaults": {
            "read_files": { "include": read_edit_scopes.clone(), "exclude": [] },
            "edit_files": { "include": read_edit_scopes, "exclude": [] },
            "create_files": { "include": [folder_scope], "exclude": [] },
            "plugin_storage": { "include": ["all"], "exclude": [] }
        },
        "reasons": {
            "read_files": "Read daily entries, index files, and optional templates from the workspace.",
            "edit_files": "Update the root index plus year, month, and daily entry files when organizing the daily hierarchy.",
            "create_files": "Create missing year, month, and daily entry files for new dates.",
            "plugin_storage": "Persist daily plugin configuration for the current workspace."
        }
    })
}

pub fn build_requested_permissions(
    folder: &str,
    root_index_path: Option<&str>,
) -> GuestRequestedPermissions {
    let perms = requested_permissions_for(folder, root_index_path);
    let defaults = perms.get("defaults").cloned().unwrap_or(JsonValue::Null);
    let reasons_value = perms.get("reasons").cloned().unwrap_or(JsonValue::Null);
    let reasons = if let Some(obj) = reasons_value.as_object() {
        obj.iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect::<HashMap<String, String>>()
    } else {
        HashMap::new()
    };
    GuestRequestedPermissions { defaults, reasons }
}

pub fn build_permissions_patch(folder: &str, root_index_path: Option<&str>) -> JsonValue {
    let defaults = requested_permissions_for(folder, root_index_path)
        .get("defaults")
        .cloned()
        .unwrap_or(JsonValue::Null);

    serde_json::json!({
        "plugin_permissions_patch": {
            "plugin_id": "diaryx.daily",
            "mode": "replace",
            "permissions": defaults
        }
    })
}

fn extract_entry_folder_update(params: &JsonValue) -> Option<Option<String>> {
    if params.get("source").and_then(|value| value.as_str()) == Some("workspace_config") {
        if params.get("field").and_then(|value| value.as_str()) != Some("daily_entry_folder") {
            return None;
        }

        let raw_value = params
            .get("value")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let normalized = normalize_folder(Some(raw_value));
        return Some((!normalized.is_empty()).then_some(normalized));
    }

    let config = params.get("config")?.as_object()?;
    let value = config.get("entry_folder")?;
    if value.is_null() {
        return Some(None);
    }

    let normalized = normalize_folder(value.as_str());
    Some((!normalized.is_empty()).then_some(normalized))
}

pub fn handle_update_config(params: JsonValue) -> Result<JsonValue, String> {
    with_state_mut(|state| {
        if let Some(next_entry_folder) = extract_entry_folder_update(&params) {
            state.config.entry_folder = next_entry_folder;
            save_workspace_config(state)?;
        }

        let folder = state.config.effective_entry_folder();
        let root_index_path = params
            .get("root_index_path")
            .and_then(|value| value.as_str())
            .or_else(|| {
                state
                    .workspace_root
                    .as_deref()
                    .filter(|value| value.ends_with(".md"))
            })
            .map(|s| s.to_string());
        Ok(build_permissions_patch(&folder, root_index_path.as_deref()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_requested_permissions_scope_daily_folder_and_root_index() {
        let permissions = requested_permissions_for("Daily", Some("README.md"));
        let read_include = permissions["defaults"]["read_files"]["include"]
            .as_array()
            .expect("read include array");
        let create_include = permissions["defaults"]["create_files"]["include"]
            .as_array()
            .expect("create include array");

        assert_eq!(read_include.len(), 2);
        assert_eq!(read_include[0].as_str(), Some("Daily"));
        assert_eq!(read_include[1].as_str(), Some("README.md"));
        assert_eq!(create_include[0].as_str(), Some("Daily"));
    }

    #[test]
    fn permissions_patch_replaces_daily_file_rules() {
        let patch = build_permissions_patch("Journal/Daily", Some("/README.md"));

        assert_eq!(
            patch["plugin_permissions_patch"]["mode"].as_str(),
            Some("replace")
        );
        assert_eq!(
            patch["plugin_permissions_patch"]["permissions"]["edit_files"]["include"][0].as_str(),
            Some("Journal/Daily")
        );
        assert_eq!(
            patch["plugin_permissions_patch"]["permissions"]["edit_files"]["include"][1].as_str(),
            Some("README.md")
        );
    }
}
