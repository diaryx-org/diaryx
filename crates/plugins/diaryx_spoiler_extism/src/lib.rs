//! Spoiler mark Extism guest plugin for Diaryx.
//!
//! Provides Discord-style `||hidden text||` spoiler syntax as an inline mark.
//! Text wrapped in double pipes is hidden until clicked.

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

// ============================================================================
// Spoiler CSS
// ============================================================================

const SPOILER_CSS: &str = r#"
.spoiler-mark {
    border-radius: 4px;
    padding: 0 2px;
    transition: all 0.2s ease;
}

.spoiler-hidden {
    background: var(--foreground);
    color: transparent;
    user-select: none;
    cursor: pointer;
}

.spoiler-revealed {
    background: var(--muted);
    color: var(--foreground);
    cursor: pointer;
}

.spoiler-hidden:hover {
    opacity: 0.8;
}
"#;

// ============================================================================
// Guest exports
// ============================================================================

/// Return the plugin manifest.
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.spoiler",
        "Spoiler",
        env!("CARGO_PKG_VERSION"),
        "Discord-style ||spoiler|| syntax to hide text until clicked",
        vec!["editor_extension".into()],
    )
    .ui(vec![serde_json::json!({
        "slot": "EditorExtension",
        "extension_id": "spoiler",
        "node_type": "InlineMark",
        "markdown": {
            "level": "Inline",
            "open": "||",
            "close": "||",
        },
        "render_export": null,
        "edit_mode": null,
        "css": SPOILER_CSS,
        "keyboard_shortcut": "Mod-Shift-s",
        "click_behavior": {
            "ToggleClass": {
                "hidden_class": "spoiler-hidden",
                "revealed_class": "spoiler-revealed",
            },
        },
        "insert_command": {
            "label": "Spoiler",
            "icon": "eye-off",
            "description": "Hide text behind a spoiler",
        },
    })]);

    Ok(serde_json::to_string(&manifest)?)
}

/// Handle commands dispatched by the host (none for this plugin).
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let request: CommandRequest = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let response = CommandResponse::err(format!("Unknown command: {}", request.command));

    Ok(serde_json::to_string(&response)?)
}

/// Handle lifecycle events (no-op for spoiler plugin).
#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

/// Get plugin configuration (none for this plugin).
#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    Ok("{}".into())
}

/// Set plugin configuration (no-op for this plugin).
#[plugin_fn]
pub fn set_config(_input: String) -> FnResult<String> {
    Ok(String::new())
}
