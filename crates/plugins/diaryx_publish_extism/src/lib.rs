//! Extism guest plugin for HTML publishing features.
//!
//! This crate compiles to a `.wasm` plugin loaded by `diaryx_extism` on native
//! and by `@extism/extism` in the web app.

pub mod host_fs;
pub mod namespace_client;
pub mod publish;
pub mod publish_plugin;
pub mod state;

use diaryx_plugin_sdk::prelude::*;
use diaryx_plugin_sdk::protocol::ServerFunctionDecl;

use extism_pdk::*;
use serde_json::Value as JsonValue;

use diaryx_core::plugin::{
    ComponentRef, HostAction, PluginCapability, PluginContext, PluginId, PluginManifest,
    SettingsField, SidebarSide, UiContribution,
};

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let sidebar = UiContribution::SidebarTab {
        id: "publish-panel".into(),
        label: "Publish".into(),
        icon: Some("send".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Declarative {
            fields: vec![
                SettingsField::HostWidget {
                    widget_id: "namespace.guard".into(),
                    sign_in_action: Some(HostAction {
                        action_type: "open-settings".into(),
                        payload: Some(serde_json::json!({ "tab": "account" })),
                    }),
                },
                SettingsField::HostWidget {
                    widget_id: "namespace.site-url".into(),
                    sign_in_action: None,
                },
                SettingsField::HostWidget {
                    widget_id: "namespace.subdomain".into(),
                    sign_in_action: None,
                },
                SettingsField::HostWidget {
                    widget_id: "namespace.custom-domains".into(),
                    sign_in_action: None,
                },
                SettingsField::HostWidget {
                    widget_id: "namespace.audiences".into(),
                    sign_in_action: None,
                },
                SettingsField::HostWidget {
                    widget_id: "namespace.publish-button".into(),
                    sign_in_action: None,
                },
            ],
        },
    };

    let pm = PluginManifest {
        id: PluginId("diaryx.publish".into()),
        name: "Publish".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "HTML rendering and website publishing".into(),
        capabilities: vec![
            PluginCapability::WorkspaceEvents,
            PluginCapability::CustomCommands {
                commands: all_commands(),
            },
        ],
        ui: vec![sidebar],
        cli: vec![],
    };

    let manifest = GuestManifest::new(
        pm.id.0,
        pm.name,
        pm.version,
        pm.description,
        vec!["workspace_events".into(), "custom_commands".into()],
    )
    .min_app_version("1.4.1")
    .ui(pm.ui.iter().map(|u| serde_json::to_value(u).unwrap_or_default()).collect())
    .commands(all_commands())
    .cli(vec![
        serde_json::json!({
            "name": "publish", "about": "Publish workspace as HTML for sharing",
            "aliases": ["pub"], "native_handler": "publish",
            "args": [
                {"name": "destination", "required": true, "help": "Destination path", "value_type": "Path"},
                {"name": "audience", "short": "a", "long": "audience", "help": "Target audience"},
                {"name": "format", "short": "F", "long": "format", "default_value": "html", "help": "Output format"},
                {"name": "single-file", "long": "single-file", "is_flag": true, "help": "Single file output"},
                {"name": "title", "short": "t", "long": "title", "help": "Site title"},
                {"name": "force", "short": "f", "long": "force", "is_flag": true, "help": "Overwrite existing"},
                {"name": "no-copy-attachments", "long": "no-copy-attachments", "is_flag": true, "help": "Skip attachments"},
                {"name": "dry-run", "long": "dry-run", "is_flag": true, "help": "Show plan only"}
            ]
        }),
        serde_json::json!({
            "name": "preview", "about": "Preview workspace as local website with live reload",
            "native_handler": "preview",
            "args": [
                {"name": "port", "short": "p", "long": "port", "default_value": "3456",
                 "value_type": "Integer", "help": "HTTP port"},
                {"name": "no-open", "long": "no-open", "is_flag": true, "help": "Don't auto-open browser"},
                {"name": "audience", "short": "a", "long": "audience", "help": "Target audience"},
                {"name": "title", "short": "t", "long": "title", "help": "Site title"}
            ]
        }),
        serde_json::json!({
            "name": "audience", "about": "Manage audience access levels for publishing",
            "aliases": ["aud"],
            "subcommands": [
                {
                    "name": "list", "about": "List audiences and their access states",
                    "command_name": "AudienceList"
                },
                {
                    "name": "set", "about": "Set an audience's access level",
                    "command_name": "AudienceSet",
                    "args": [
                        {"name": "name", "required": true, "help": "Audience name"},
                        {"name": "state", "short": "s", "long": "state", "required": true,
                         "help": "Access state: public, token, or unpublished"}
                    ]
                },
                {
                    "name": "remove", "about": "Unpublish an audience",
                    "command_name": "AudienceRemove",
                    "args": [
                        {"name": "name", "required": true, "help": "Audience name"}
                    ]
                }
            ]
        }),
    ])
    .server_functions(vec![
        ServerFunctionDecl {
            name: "put_object".into(),
            method: "PUT".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Upload a published HTML artifact or attachment to a namespace".into(),
        },
        ServerFunctionDecl {
            name: "get_object".into(),
            method: "GET".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Retrieve a workspace object (e.g. CRDT state or attachment)".into(),
        },
        ServerFunctionDecl {
            name: "list_objects".into(),
            method: "GET".into(),
            path: "/namespaces/{id}/objects".into(),
            description: "List object metadata in a namespace (used to prune stale artifacts)".into(),
        },
        ServerFunctionDecl {
            name: "delete_object".into(),
            method: "DELETE".into(),
            path: "/namespaces/{id}/objects/{key}".into(),
            description: "Delete a stale published artifact from a namespace".into(),
        },
        ServerFunctionDecl {
            name: "send_audience_email".into(),
            method: "POST".into(),
            path: "/namespaces/{id}/audiences/{audience}/send-email".into(),
            description: "Send the email draft for an audience to all subscribers".into(),
        },
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] }
        }),
        reasons: [
            ("read_files".into(), "Read workspace entries and attachments for publishing.".into()),
            ("edit_files".into(), "Update publish config in workspace frontmatter.".into()),
            ("create_files".into(), "Create published HTML output files.".into()),
        ].into_iter().collect(),
    });

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or(InitParams {
        workspace_root: None,
    });

    state::init_state().map_err(extism_pdk::Error::msg)?;

    if let Some(root) = params.workspace_root {
        let root_path = std::path::PathBuf::from(&root);
        let init_result = state::with_state(|s| {
            let ctx = PluginContext {
                workspace_root: Some(root_path.clone()),
                link_format: diaryx_core::link_parser::LinkFormat::default(),
            };
            poll_future(diaryx_core::plugin::Plugin::init(&s.publish_plugin, &ctx))
        })
        .map_err(extism_pdk::Error::msg)?;
        init_result.map_err(extism_pdk::Error::msg)?;

        // Trigger workspace_opened so load_config reads frontmatter.
        // The browser host does not send a workspace_opened event, so
        // we fire it here during init to ensure config is loaded.
        let _ = state::with_state(|s| {
            let event = diaryx_core::plugin::WorkspaceOpenedEvent {
                workspace_root: root_path,
            };
            poll_future(diaryx_core::plugin::WorkspacePlugin::on_workspace_opened(
                &s.publish_plugin,
                &event,
            ));
        });
    }

    host::log::log("info", "Publish plugin initialized");
    Ok(String::new())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    if let Err(e) = state::shutdown_state() {
        host::log::log("warn", &format!("Shutdown state cleanup failed: {e}"));
    }
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;

    // Handle CLI audience commands by mapping to core plugin commands.
    let response = match req.command.as_str() {
        "AudienceList" => handle_audience_list(),
        "AudienceSet" => handle_audience_set(&req.params),
        "AudienceRemove" => handle_audience_remove(&req.params),
        _ => {
            let result = state::with_state(|s| {
                poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
                    &s.publish_plugin,
                    &req.command,
                    req.params,
                ))
            });

            match result {
                Ok(Some(Ok(data))) => CommandResponse::ok(data),
                Ok(Some(Err(e))) => CommandResponse::err(e.to_string()),
                Ok(None) => CommandResponse::err(format!("Unknown command: {}", req.command)),
                Err(e) => CommandResponse::err(e),
            }
        }
    };

    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: GuestEvent = serde_json::from_str(&input)?;

    if event.event_type == "workspace_opened"
        && let Some(root) = event.payload.get("workspace_root").and_then(|v| v.as_str())
    {
        let _ = state::with_state(|s| {
            let event = diaryx_core::plugin::WorkspaceOpenedEvent {
                workspace_root: std::path::PathBuf::from(root),
            };
            poll_future(diaryx_core::plugin::WorkspacePlugin::on_workspace_opened(
                &s.publish_plugin,
                &event,
            ));
        });
    }

    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = match state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::get_config(
            &s.publish_plugin,
        ))
    }) {
        Ok(c) => c,
        Err(_) => None,
    };

    match config {
        Some(value) => Ok(serde_json::to_string(&value)?),
        None => Ok("{}".into()),
    }
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: JsonValue = serde_json::from_str(&input)?;
    let _ = state::with_state(|s| {
        let _ = poll_future(diaryx_core::plugin::WorkspacePlugin::set_config(
            &s.publish_plugin,
            config,
        ));
    });
    Ok(String::new())
}

/// Execute a typed Command (same format as Diaryx::execute).
///
/// Takes a JSON object with `type` and optional `params` fields, extracts
/// them, and calls `handle_command` on the inner PublishPlugin.
/// Returns the result as a serialized JSON string.
/// Returns empty string if the command is not handled by this plugin.
#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    let parsed: serde_json::Value = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid JSON: {e}")))?;

    let cmd_type = parsed["type"]
        .as_str()
        .ok_or_else(|| extism_pdk::Error::msg("Missing 'type' field in command"))?;

    let params = parsed
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let result = state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
            &s.publish_plugin,
            cmd_type,
            params,
        ))
    })
    .map_err(|e| extism_pdk::Error::msg(e))?;

    match result {
        Some(Ok(value)) => {
            let response = serde_json::json!({ "type": "PluginResult", "data": value });
            let json = serde_json::to_string(&response)
                .map_err(|e| extism_pdk::Error::msg(format!("Serialize error: {e}")))?;
            Ok(json)
        }
        Some(Err(e)) => Err(extism_pdk::Error::msg(format!("{e}")).into()),
        None => Ok(String::new()),
    }
}

// ── CLI audience command handlers ─────────────────────────────────────

fn delegate_command(command: &str, params: JsonValue) -> Result<JsonValue, String> {
    state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
            &s.publish_plugin,
            command,
            params,
        ))
    })
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("Command {command} not handled"))?
    .map_err(|e| e.to_string())
}

fn cli_print(text: &str) -> CommandResponse {
    CommandResponse::ok(serde_json::json!({
        "__diaryx_cli_action": "print",
        "text": text
    }))
}

fn handle_audience_list() -> CommandResponse {
    match delegate_command("GetAudiencePublishStates", JsonValue::Null) {
        Ok(data) => {
            let map = match data.as_object() {
                Some(m) => m,
                None => return cli_print("No audiences configured."),
            };
            if map.is_empty() {
                return cli_print("No audiences configured.");
            }

            let mut lines = vec![format!("{:<24} {}", "AUDIENCE", "STATE")];
            lines.push("-".repeat(44));
            for (name, config) in map {
                let state = config
                    .get("state")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                lines.push(format!("{:<24} {}", name, state));
            }
            lines.push(String::new());
            lines.push(format!("{} audience(s) total.", map.len()));
            cli_print(&lines.join("\n"))
        }
        Err(e) => CommandResponse::err(format!("Failed to list audiences: {e}")),
    }
}

fn handle_audience_set(params: &JsonValue) -> CommandResponse {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CommandResponse::err("Missing audience name"),
    };
    let state_str = match params.get("state").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return CommandResponse::err(
                "Missing --state argument (public, token, or unpublished)",
            );
        }
    };

    let state = match state_str {
        "public" | "Public" => "Public",
        "token" | "access-control" | "AccessControl" => "AccessControl",
        "unpublished" | "Unpublished" => "Unpublished",
        other => {
            return CommandResponse::err(format!(
                "Invalid state '{other}'. Use: public, token, or unpublished"
            ));
        }
    };

    let set_params = serde_json::json!({
        "audience": name,
        "config": { "state": state }
    });

    match delegate_command("SetAudiencePublishState", set_params) {
        Ok(_) => cli_print(&format!("Audience '{name}' set to {state_str}.")),
        Err(e) => CommandResponse::err(format!("Failed to set audience: {e}")),
    }
}

fn handle_audience_remove(params: &JsonValue) -> CommandResponse {
    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return CommandResponse::err("Missing audience name"),
    };

    let set_params = serde_json::json!({
        "audience": name,
        "config": { "state": "Unpublished" }
    });

    match delegate_command("SetAudiencePublishState", set_params) {
        Ok(_) => cli_print(&format!("Audience '{name}' removed (unpublished).")),
        Err(e) => CommandResponse::err(format!("Failed to remove audience: {e}")),
    }
}

fn all_commands() -> Vec<String> {
    [
        "OpenPublishPanel",
        "GetPublishConfig",
        "SetPublishConfig",
        "GetAudiencePublishStates",
        "SetAudiencePublishState",
        "PublishToNamespace",
        "PublishWorkspace",
        "AudienceList",
        "AudienceSet",
        "AudienceRemove",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn poll_future<F: std::future::Future>(f: F) -> F::Output {
    use std::pin::pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );

    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);
    let mut pinned = pin!(f);

    match pinned.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("Future was not immediately ready in Extism guest"),
    }
}
