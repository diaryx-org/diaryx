use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

const DRAWING_CANVAS_HTML: &str = include_str!("drawing_canvas.html");

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.drawing",
        "Drawing",
        env!("CARGO_PKG_VERSION"),
        "Freehand drawing blocks with pen, eraser, colors, and undo/redo.",
        vec!["editor_extension".into()],
    )
    .ui(vec![
        // Editor extension: iframe-based drawing block
        serde_json::json!({
            "slot": "EditorExtension",
            "extension_id": "drawingBlock",
            "node_type": "BlockAtom",
            "markdown": {
                "level": "Block",
                "open": "![drawing:",
                "close": ")",
                "single_line": true,
            },
            "render_export": null,
            "edit_mode": "Iframe",
            "iframe_component_id": "drawing-canvas",
            "css": null,
            "insert_command": {
                "label": "Drawing",
                "icon": "pencil",
                "description": "Insert a freehand drawing",
            },
        }),
    ])
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
                "drawing-canvas" => {
                    CommandResponse::ok(serde_json::json!({ "html": DRAWING_CANVAS_HTML }))
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
