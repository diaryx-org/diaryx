//! Permission scopes, manifest defaults, and patch generation.

use std::collections::HashMap;

use diaryx_plugin_sdk::prelude::*;
use serde_json::Value as JsonValue;

use crate::paths::{normalize_rel_path, root_index_scope};

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

/// Build the permission request the host surfaces (and clamps to the manifest
/// ceiling) when the daily folder changes. The folder-scoped read/edit/create
/// rules are derived from the effective folder + workspace root index.
pub fn build_permission_request(folder: &str, root_index_path: Option<&str>) -> PermissionRequest {
    let perms = requested_permissions_for(folder, root_index_path);
    let permissions = perms.get("defaults").cloned().unwrap_or(JsonValue::Null);
    let reasons = perms
        .get("reasons")
        .and_then(|value| value.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect::<HashMap<String, String>>()
        })
        .unwrap_or_default();
    PermissionRequest {
        permissions,
        reasons,
    }
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
    fn permission_request_carries_folder_scope_and_reasons() {
        let request = build_permission_request("Journal/Daily", Some("/README.md"));

        let edit_include = request.permissions["edit_files"]["include"]
            .as_array()
            .expect("edit include array");
        assert_eq!(edit_include[0].as_str(), Some("Journal/Daily"));
        assert_eq!(edit_include[1].as_str(), Some("README.md"));
        assert!(request.reasons.contains_key("read_files"));
    }
}
