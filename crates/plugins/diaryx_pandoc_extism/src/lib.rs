//! Extism guest plugin for pandoc-based format conversion and export.
//!
//! This crate compiles to a `.wasm` plugin loaded by `diaryx_extism` on native
//! and by `@extism/extism` in the web app.

pub mod converter;
pub mod host_fs;
pub mod state;

use diaryx_plugin_sdk::prelude::*;

diaryx_plugin_sdk::register_getrandom_v02!();

use extism_pdk::*;

use diaryx_core::export::Exporter;
use diaryx_core::plugin::{PluginCapability, PluginId, PluginManifest, UiContribution};

use crate::host_fs::HostFs;

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let palette_export = UiContribution::CommandPaletteItem {
        id: "pandoc-export".into(),
        label: "Export...".into(),
        group: Some("Export".into()),
        plugin_command: "OpenExportDialog".into(),
    };

    let pm = PluginManifest {
        id: PluginId("diaryx.pandoc".into()),
        name: "Pandoc".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Multi-format export via pandoc WASM".into(),
        capabilities: vec![
            PluginCapability::WorkspaceEvents,
            PluginCapability::CustomCommands {
                commands: all_commands(),
            },
        ],
        ui: vec![palette_export],
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
    .ui(pm
        .ui
        .iter()
        .map(|u| serde_json::to_value(u).unwrap_or_default())
        .collect())
    .commands(all_commands())
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "read_files": { "include": ["all"], "exclude": [] },
            "http_requests": { "include": ["unpkg.com"], "exclude": [] },
            // The pandoc WASM module is ~58 MB; request 100 MiB so the
            // download fits with headroom. The host caps this at its hard
            // ceiling regardless.
            "plugin_storage": { "include": ["all"], "exclude": [], "quota_bytes": 104857600u64 }
        }),
        reasons: [
            (
                "read_files".into(),
                "Read workspace entries for export and format conversion.".into(),
            ),
            (
                "http_requests".into(),
                "Download pandoc WASM module for format conversion.".into(),
            ),
            (
                "plugin_storage".into(),
                "Cache the ~58 MB pandoc WASM module between runs.".into(),
            ),
        ]
        .into_iter()
        .collect(),
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
        state::with_state_mut(|s| {
            s.workspace_root = Some(std::path::PathBuf::from(&root));
        })
        .map_err(extism_pdk::Error::msg)?;
    }

    // The pandoc WASM is ~58 MB and must be downloaded over HTTP. We used
    // to fetch it here to pre-warm the cache, but that blocked init for
    // 20+ seconds and held up every other plugin's readiness reporting on
    // hosts that init plugins serially. `convert_format` already
    // downloads on demand, so init now returns immediately and the cost
    // moves to the first conversion (when the user explicitly asked for
    // it). Hosts that want eager warming can dispatch `DownloadConverter`
    // themselves.

    host::log::log("info", "Pandoc plugin initialized");
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

    let response = match req.command.as_str() {
        "ConvertFormat" => {
            let content = req
                .params
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let from = req
                .params
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("markdown");
            let to = req
                .params
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("html");
            let resources: Option<std::collections::HashMap<String, String>> = req
                .params
                .get("resources")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok());

            match converter::convert_format(content, from, to, resources.as_ref()) {
                Ok(result) => CommandResponse::ok(serde_json::to_value(result).unwrap_or_default()),
                Err(e) => CommandResponse::err(e),
            }
        }
        "ConvertToPdf" => {
            let content = req
                .params
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let from = req
                .params
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("markdown");
            let resources: Option<std::collections::HashMap<String, String>> = req
                .params
                .get("resources")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok());

            match converter::convert_format(content, from, "pdf", resources.as_ref()) {
                Ok(result) => CommandResponse::ok(serde_json::to_value(result).unwrap_or_default()),
                Err(e) => CommandResponse::err(e),
            }
        }
        "DownloadConverter" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("pandoc");
            match converter::download_converter(name) {
                Ok(()) => CommandResponse::ok(serde_json::json!({ "ok": true })),
                Err(e) => CommandResponse::err(e),
            }
        }
        "IsConverterAvailable" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("pandoc");
            let available = converter::is_converter_available(name);
            CommandResponse::ok(serde_json::json!({ "available": available }))
        }
        "GetExportFormats" => CommandResponse::ok(
            serde_json::to_value(converter::get_export_formats()).unwrap_or_default(),
        ),
        "PlanExport" => {
            let root_path = match req.params.get("root_path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => {
                    return Ok(serde_json::to_string(&CommandResponse::err(
                        "missing root_path",
                    ))?);
                }
            };
            let audience = req
                .params
                .get("audience")
                .and_then(|v| v.as_str())
                .unwrap_or("*");
            let resolved = resolve_path(root_path);
            let default_aud = read_default_audience();
            let exporter = Exporter::new(HostFs);
            match poll_future(exporter.plan_export(
                &resolved,
                audience,
                std::path::Path::new("/tmp/export"),
                default_aud.as_deref(),
            )) {
                Ok(plan) => CommandResponse::ok(serde_json::to_value(plan).unwrap_or_default()),
                Err(e) => CommandResponse::err(e.to_string()),
            }
        }
        "ExportToMemory" => {
            let root_path = match req.params.get("root_path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => {
                    return Ok(serde_json::to_string(&CommandResponse::err(
                        "missing root_path",
                    ))?);
                }
            };
            let audience = req
                .params
                .get("audience")
                .and_then(|v| v.as_str())
                .unwrap_or("*");
            let resolved = resolve_path(root_path);
            let default_aud = read_default_audience();
            let exporter = Exporter::new(HostFs);

            match poll_future(exporter.export_to_memory(
                &resolved,
                audience,
                default_aud.as_deref(),
            )) {
                Ok(mut files) => {
                    // Apply visibility filtering first, then optionally delegate
                    // remaining Handlebars-style body templating to the templating plugin.
                    for file in &mut files {
                        let visibility_processed = if audience != "*" {
                            diaryx_core::visibility::filter_body_for_audience(
                                &file.content,
                                audience,
                            )
                        } else {
                            diaryx_core::visibility::strip_visibility_directives(&file.content)
                        };

                        if body_needs_template_render(&visibility_processed) {
                            let mut params = serde_json::json!({
                                "body": visibility_processed,
                                "file_path": file.path,
                            });
                            if audience != "*" {
                                params["audience"] = serde_json::Value::String(audience.into());
                            }
                            if let Ok(rendered) =
                                host::plugins::call("diaryx.templating", "RenderBody", params)
                                && let Some(s) = rendered.as_str()
                            {
                                file.content = s.to_string();
                                continue;
                            }
                        }

                        file.content = visibility_processed;
                    }
                    CommandResponse::ok(serde_json::to_value(files).unwrap_or_default())
                }
                Err(e) => CommandResponse::err(e.to_string()),
            }
        }
        "ExportBinaryAttachments" => {
            let root_path = match req.params.get("root_path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => {
                    return Ok(serde_json::to_string(&CommandResponse::err(
                        "missing root_path",
                    ))?);
                }
            };
            let resolved = resolve_path(root_path);
            let exporter = Exporter::new(HostFs);
            let attachments = poll_future(exporter.collect_binary_attachments(&resolved));
            CommandResponse::ok(serde_json::to_value(attachments).unwrap_or_default())
        }
        "OpenExportDialog" => {
            // The host handles the UI — just acknowledge.
            CommandResponse::ok(serde_json::json!({ "action": "open-export-dialog" }))
        }
        _ => CommandResponse::err(format!("Unknown command: {}", req.command)),
    };

    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: GuestEvent = serde_json::from_str(&input)?;

    if event.event_type == "workspace_opened"
        && let Some(root) = event.payload.get("workspace_root").and_then(|v| v.as_str())
    {
        let _ = state::with_state_mut(|s| {
            s.workspace_root = Some(std::path::PathBuf::from(root));
        });
    }

    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    Ok("{}".into())
}

#[plugin_fn]
pub fn set_config(_input: String) -> FnResult<String> {
    Ok(String::new())
}

fn all_commands() -> Vec<String> {
    [
        "PlanExport",
        "ExportToMemory",
        "ExportBinaryAttachments",
        "GetExportFormats",
        "DownloadConverter",
        "IsConverterAvailable",
        "ConvertFormat",
        "ConvertToPdf",
        "OpenExportDialog",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn body_needs_template_render(body: &str) -> bool {
    let params = serde_json::json!({ "body": body });
    host::plugins::call("diaryx.templating", "HasTemplates", params)
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| body.contains("{{"))
}

/// Resolve a workspace-relative path against the workspace root.
fn resolve_path(path: &str) -> std::path::PathBuf {
    match state::with_state(|s| s.workspace_root.clone()) {
        Ok(Some(root)) => root.join(path),
        _ => std::path::PathBuf::from(path),
    }
}

/// Read default_audience from workspace config.
fn read_default_audience() -> Option<String> {
    let root = state::with_state(|s| s.workspace_root.clone()).ok()??;
    let ws = diaryx_core::workspace::Workspace::new(HostFs);
    poll_future(ws.get_workspace_config(&root))
        .ok()
        .and_then(|c| c.default_audience)
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
