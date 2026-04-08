use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

const AUDIO_RECORDER_HTML: &str = include_str!("audio_recorder.html");

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.audio",
        "Audio Recording",
        env!("CARGO_PKG_VERSION"),
        "Record audio blocks with waveform visualization and playback.",
        vec!["editor_extension".into()],
    )
    .ui(vec![serde_json::json!({
        "slot": "EditorExtension",
        "extension_id": "audioBlock",
        "node_type": "BlockAtom",
        "markdown": {
            "level": "Block",
            "open": "![audio:",
            "close": ")",
            "single_line": true,
        },
        "render_export": null,
        "edit_mode": "Iframe",
        "iframe_component_id": "audio-recorder",
        "css": null,
        "insert_command": {
            "label": "Audio Recording",
            "icon": "mic",
            "description": "Insert an audio recording",
        },
        "host_capabilities": ["audio_capture"],
    })])
    .commands(vec!["get_component_html".into()])
    .min_app_version("1.4.0");
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let resp = match req.command.as_str() {
        "get_component_html" => {
            let component_id = req
                .params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match component_id {
                "audio-recorder" => {
                    CommandResponse::ok(serde_json::json!({ "html": AUDIO_RECORDER_HTML }))
                }
                _ => CommandResponse::err(format!("Unknown component: {component_id}")),
            }
        }
        _ => CommandResponse::err(format!("Unknown command: {}", req.command)),
    };
    Ok(serde_json::to_string(&resp)?)
}

#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
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
