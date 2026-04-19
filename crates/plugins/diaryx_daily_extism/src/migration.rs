//! One-time migration of legacy workspace frontmatter keys into plugin storage.

use diaryx_core::yaml_value::YamlValue;
use diaryx_plugin_sdk::prelude::*;

use crate::markdown_io::{parse_markdown, write_markdown};
use crate::paths::find_root_index_candidates;
use crate::state::DailyState;
use crate::storage::save_workspace_config;

pub fn migrate_legacy_config(state_value: &mut DailyState) -> Result<(), String> {
    if state_value.config.migrated_legacy_config {
        return Ok(());
    }

    let mut migrated_any = false;

    for candidate in find_root_index_candidates(state_value.workspace_root.as_deref()) {
        if !host::fs::file_exists(&candidate)? {
            continue;
        }

        let content = host::fs::read_file(&candidate)?;
        let (mut fm, body) = parse_markdown(&content)?;
        let mut file_changed = false;

        if let Some(YamlValue::String(folder)) = fm.shift_remove("daily_entry_folder") {
            if state_value
                .config
                .entry_folder
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                state_value.config.entry_folder = Some(folder);
            }
            file_changed = true;
            migrated_any = true;
        }

        if let Some(YamlValue::String(template)) = fm.shift_remove("daily_template") {
            if state_value
                .config
                .entry_template
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(true)
            {
                state_value.config.entry_template = Some(template);
            }
            file_changed = true;
            migrated_any = true;
        }

        if file_changed {
            write_markdown(&candidate, &fm, &body)?;
        }
    }

    if migrated_any {
        host::log::log(
            "info",
            "Migrated legacy daily workspace keys into plugin config",
        );
    }

    state_value.config.migrated_legacy_config = true;
    save_workspace_config(state_value)?;
    Ok(())
}
