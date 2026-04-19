//! Extism guest plugin for Diaryx daily entry functionality.
//!
//! This file is intentionally slim — it contains only the `#[plugin_fn]`
//! entry points and module declarations. Logic lives in submodules:
//!
//! - `commands` — command dispatch
//! - `daily_logic` — pure date/path/template domain
//! - `indices` — daily index files, contents/part_of maintenance, tree walks
//! - `links` — link format discovery and formatting
//! - `markdown_io` — frontmatter read/write helpers
//! - `migration` — one-time legacy-config migration
//! - `paths` — filesystem/workspace path conversion
//! - `permissions` — manifest permissions and runtime patches
//! - `state` — plugin thread-local state and workspace lifecycle
//! - `storage` — workspace-scoped plugin config persistence

mod commands;
mod daily_logic;
mod indices;
mod links;
mod markdown_io;
mod migration;
mod paths;
mod permissions;
mod state;
mod storage;

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use serde_json::Value as JsonValue;

use commands::{all_commands, dispatch_command, get_component_html_by_id};
use daily_logic::DailyPluginConfig;
use permissions::build_requested_permissions;
use state::{InitParams, current_state, update_workspace_root, with_state_mut};
use storage::save_workspace_config;

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.daily",
        "Daily",
        env!("CARGO_PKG_VERSION"),
        "Daily entry plugin with date hierarchy, navigation, and CLI surface",
        vec!["workspace_events".into(), "custom_commands".into()],
    )
    .ui(vec![
        serde_json::json!({
            "slot": "SidebarTab",
            "id": "daily-panel",
            "label": "Daily",
            "icon": "calendar-days",
            "side": "Left",
            "component": {
                "type": "Iframe",
                "component_id": "daily.panel",
            },
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "daily-open-today",
            "label": "Open Today's Entry",
            "group": "Daily",
            "plugin_command": "OpenToday",
        }),
        serde_json::json!({
            "slot": "CommandPaletteItem",
            "id": "daily-open-yesterday",
            "label": "Open Yesterday's Entry",
            "group": "Daily",
            "plugin_command": "OpenYesterday",
        }),
    ])
    .commands(all_commands())
    .cli(vec![serde_json::json!({
        "name": "daily",
        "about": "Open or print a daily entry",
        "aliases": ["d"],
        "command_name": "CliDaily",
        "requires_workspace": true,
        "args": [
            {
                "name": "date",
                "help": "Date expression (today, yesterday, YYYY-MM-DD)",
                "required": false,
                "value_type": "String"
            },
            {
                "name": "print",
                "help": "Print entry content instead of launching editor",
                "short": "p",
                "long": "print",
                "is_flag": true
            }
        ]
    })])
    .requested_permissions(build_requested_permissions(
        &DailyPluginConfig::default().effective_entry_folder(),
        Some("README.md"),
    ));

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    update_workspace_root(params.workspace_root).map_err(extism_pdk::Error::msg)?;
    host::log::log("info", "Daily plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host::log::log("info", "Daily plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = match dispatch_command(&req.command, req.params) {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
    };

    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    let parsed: JsonValue = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid JSON: {e}")))?;

    let cmd_type = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| extism_pdk::Error::msg("Missing `type` in typed command"))?;

    let params = parsed.get("params").cloned().unwrap_or(JsonValue::Null);
    match dispatch_command(cmd_type, params) {
        Ok(data) => {
            let response = serde_json::json!({
                "type": "PluginResult",
                "data": data
            });
            Ok(serde_json::to_string(&response)?)
        }
        Err(_) => Ok(String::new()),
    }
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let state = current_state().map_err(extism_pdk::Error::msg)?;
    Ok(serde_json::to_string(&state.config)?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    with_state_mut(|state| {
        let mut config: DailyPluginConfig = serde_json::from_str(&input).unwrap_or_default();

        if let Some(folder) = config.entry_folder.as_deref() {
            config.entry_folder = Some(folder.trim_matches('/').to_string());
        }

        state.config.entry_folder = config.entry_folder;
        state.config.entry_template = config.entry_template;
        if config.migrated_legacy_config {
            state.config.migrated_legacy_config = true;
        }

        save_workspace_config(state)?;
        Ok(())
    })
    .map_err(extism_pdk::Error::msg)?;
    Ok(String::new())
}

#[plugin_fn]
pub fn get_component_html(input: String) -> FnResult<String> {
    if input.trim().is_empty() {
        return Ok(include_str!("ui/panel.html").to_string());
    }

    if input.trim_start().starts_with('{') {
        let parsed: JsonValue = serde_json::from_str(&input)?;
        let component_id = parsed
            .get("component_id")
            .and_then(|v| v.as_str())
            .unwrap_or("daily.panel");
        if let Some(html) = get_component_html_by_id(component_id) {
            return Ok(html.to_string());
        }
        return Err(extism_pdk::Error::msg(format!("Unknown component id: {component_id}")).into());
    }

    if let Some(html) = get_component_html_by_id(input.trim()) {
        return Ok(html.to_string());
    }

    Err(extism_pdk::Error::msg(format!("Unknown component id: {}", input.trim())).into())
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: JsonValue = serde_json::from_str(&input).unwrap_or(JsonValue::Null);
    let event_type = event
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if matches!(event_type, "workspace_opened" | "workspace_changed") {
        let workspace_root = event
            .get("payload")
            .and_then(|v| v.get("workspace_root"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let _ = update_workspace_root(workspace_root);
    }

    Ok(String::new())
}
