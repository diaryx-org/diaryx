//! Extism guest plugin for publish/export features.
//!
//! This crate compiles to a `.wasm` plugin loaded by `diaryx_extism` on native
//! and by `@extism/extism` in the web app.

pub mod converter;
pub mod host_bridge;
pub mod host_fs;
pub mod state;

mod custom_random {
    use std::sync::atomic::{AtomicU64, Ordering};

    static RNG_STATE: AtomicU64 = AtomicU64::new(0);

    fn xorshift_fill(buf: &mut [u8]) {
        let mut state = RNG_STATE.load(Ordering::Relaxed);
        if state == 0 {
            state = crate::host_bridge::get_timestamp().unwrap_or(42);
            if state == 0 {
                state = 42;
            }
        }
        for byte in buf.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        RNG_STATE.store(state, Ordering::Relaxed);
    }

    fn custom_getrandom_v02(buf: &mut [u8]) -> Result<(), getrandom::Error> {
        xorshift_fill(buf);
        Ok(())
    }

    getrandom::register_custom_getrandom!(custom_getrandom_v02);

    #[unsafe(no_mangle)]
    unsafe extern "Rust" fn __getrandom_v03_custom(
        dest: *mut u8,
        len: usize,
    ) -> Result<(), getrandom_03::Error> {
        unsafe {
            let buf = core::slice::from_raw_parts_mut(dest, len);
            xorshift_fill(buf);
        }
        Ok(())
    }
}

use extism_pdk::*;
use serde_json::Value as JsonValue;

use diaryx_core::plugin::{
    ComponentRef, PluginCapability, PluginContext, PluginId, PluginManifest, SidebarSide,
    UiContribution,
};

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestManifest {
    id: String,
    name: String,
    version: String,
    description: String,
    capabilities: Vec<String>,
    #[serde(default)]
    ui: Vec<JsonValue>,
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestEvent {
    event_type: String,
    payload: JsonValue,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandRequest {
    command: String,
    params: JsonValue,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CommandResponse {
    success: bool,
    #[serde(default)]
    data: Option<JsonValue>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let sidebar = UiContribution::SidebarTab {
        id: "publish-panel".into(),
        label: "Publish".into(),
        icon: Some("send".into()),
        side: SidebarSide::Left,
        component: ComponentRef::Builtin {
            component_id: "publish.panel".into(),
        },
    };

    let palette_export = UiContribution::CommandPaletteItem {
        id: "publish-export".into(),
        label: "Export...".into(),
        group: Some("Publish".into()),
        plugin_command: "OpenExportDialog".into(),
    };

    let palette_publish = UiContribution::CommandPaletteItem {
        id: "publish-site".into(),
        label: "Publish Site".into(),
        group: Some("Publish".into()),
        plugin_command: "OpenPublishPanel".into(),
    };

    let pm = PluginManifest {
        id: PluginId("publish".into()),
        name: "Publish".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Export and publish content with optional format conversion".into(),
        capabilities: vec![
            PluginCapability::WorkspaceEvents,
            PluginCapability::CustomCommands {
                commands: all_commands(),
            },
        ],
        ui: vec![sidebar, palette_export, palette_publish],
    };

    let manifest = GuestManifest {
        id: pm.id.0,
        name: pm.name,
        version: pm.version,
        description: pm.description,
        capabilities: vec!["workspace_events".into(), "custom_commands".into()],
        ui: vec![
            serde_json::to_value(&pm.ui[0]).unwrap_or_default(),
            serde_json::to_value(&pm.ui[1]).unwrap_or_default(),
            serde_json::to_value(&pm.ui[2]).unwrap_or_default(),
        ],
        commands: all_commands(),
    };

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or(InitParams {
        workspace_root: None,
    });

    state::init_state().map_err(extism_pdk::Error::msg)?;

    if let Some(root) = params.workspace_root {
        let init_result = state::with_state(|s| {
            let ctx = PluginContext {
                workspace_root: Some(std::path::PathBuf::from(root)),
                link_format: diaryx_core::link_parser::LinkFormat::default(),
            };
            poll_future(diaryx_core::plugin::Plugin::init(&s.publish_plugin, &ctx))
        })
        .map_err(extism_pdk::Error::msg)?;
        init_result.map_err(extism_pdk::Error::msg)?;
    }

    host_bridge::log_message("info", "Publish plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    if let Err(e) = state::shutdown_state() {
        host_bridge::log_message("warn", &format!("Shutdown state cleanup failed: {e}"));
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
                Ok(result) => CommandResponse {
                    success: true,
                    data: Some(serde_json::to_value(result).unwrap_or_default()),
                    error: None,
                },
                Err(e) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                },
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
                Ok(result) => CommandResponse {
                    success: true,
                    data: Some(serde_json::to_value(result).unwrap_or_default()),
                    error: None,
                },
                Err(e) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                },
            }
        }
        "DownloadConverter" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("pandoc");
            match converter::download_converter(name) {
                Ok(()) => CommandResponse {
                    success: true,
                    data: Some(serde_json::json!({ "ok": true })),
                    error: None,
                },
                Err(e) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                },
            }
        }
        "IsConverterAvailable" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("pandoc");
            let available = converter::is_converter_available(name);
            CommandResponse {
                success: true,
                data: Some(serde_json::json!({ "available": available })),
                error: None,
            }
        }
        "GetExportFormats" => CommandResponse {
            success: true,
            data: Some(serde_json::to_value(converter::get_export_formats()).unwrap_or_default()),
            error: None,
        },
        _ => {
            let result = state::with_state(|s| {
                poll_future(diaryx_core::plugin::WorkspacePlugin::handle_command(
                    &s.publish_plugin,
                    &req.command,
                    req.params,
                ))
            });

            match result {
                Ok(Some(Ok(data))) => CommandResponse {
                    success: true,
                    data: Some(data),
                    error: None,
                },
                Ok(Some(Err(e))) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                },
                Ok(None) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(format!("Unknown command: {}", req.command)),
                },
                Err(e) => CommandResponse {
                    success: false,
                    data: None,
                    error: Some(e),
                },
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
/// Takes a full serialized Command JSON, calls handle_typed_command on the
/// inner PublishPlugin, and returns a full serialized Response JSON.
/// Returns empty string if the command is not handled by this plugin.
#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    use diaryx_core::command::Command;

    let cmd: Command = serde_json::from_str(&input)
        .map_err(|e| extism_pdk::Error::msg(format!("Invalid command: {e}")))?;

    let result = state::with_state(|s| {
        poll_future(diaryx_core::plugin::WorkspacePlugin::handle_typed_command(
            &s.publish_plugin,
            &cmd,
        ))
    })
    .map_err(|e| extism_pdk::Error::msg(e))?;

    match result {
        Some(Ok(response)) => {
            let json = serde_json::to_string(&response)
                .map_err(|e| extism_pdk::Error::msg(format!("Serialize error: {e}")))?;
            Ok(json)
        }
        Some(Err(e)) => Err(extism_pdk::Error::msg(format!("{e}")).into()),
        None => Ok(String::new()),
    }
}

fn all_commands() -> Vec<String> {
    [
        "PlanExport",
        "GetAvailableAudiences",
        "ExportToMemory",
        "ExportToHtml",
        "ExportBinaryAttachments",
        "GetExportFormats",
        "DownloadConverter",
        "IsConverterAvailable",
        "ConvertFormat",
        "ConvertToPdf",
        "OpenExportDialog",
        "OpenPublishPanel",
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
