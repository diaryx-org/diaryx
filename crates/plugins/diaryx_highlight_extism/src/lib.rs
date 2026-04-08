//! Colored highlight mark Extism guest plugin for Diaryx.
//!
//! Provides `==text==` and `=={color}text==` colored highlight syntax as an
//! inline mark. Supports 10 predefined colors with light and dark mode styles.

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

// ============================================================================
// Highlight CSS
// ============================================================================

const HIGHLIGHT_CSS: &str = r#"
.highlight-mark {
    border-radius: 2px;
    padding: 0 2px;
}

/* Light mode highlight colors */
.highlight-red { background: oklch(0.92 0.12 25); }
.highlight-orange { background: oklch(0.93 0.1 60); }
.highlight-yellow { background: oklch(0.95 0.12 95); }
.highlight-green { background: oklch(0.92 0.08 145); }
.highlight-cyan { background: oklch(0.92 0.08 195); }
.highlight-blue { background: oklch(0.88 0.1 250); }
.highlight-violet { background: oklch(0.9 0.1 300); }
.highlight-pink { background: oklch(0.93 0.1 350); }
.highlight-brown { background: oklch(0.88 0.06 60); }
.highlight-grey { background: oklch(0.9 0 0); }

/* Dark mode highlight colors */
.dark .highlight-red { background: oklch(0.35 0.12 25); }
.dark .highlight-orange { background: oklch(0.38 0.1 60); }
.dark .highlight-yellow { background: oklch(0.42 0.12 95); }
.dark .highlight-green { background: oklch(0.38 0.08 145); }
.dark .highlight-cyan { background: oklch(0.38 0.08 195); }
.dark .highlight-blue { background: oklch(0.35 0.1 250); }
.dark .highlight-violet { background: oklch(0.38 0.1 300); }
.dark .highlight-pink { background: oklch(0.4 0.1 350); }
.dark .highlight-brown { background: oklch(0.38 0.06 60); }
.dark .highlight-grey { background: oklch(0.4 0 0); }
"#;

// ============================================================================
// Guest exports
// ============================================================================

/// Return the plugin manifest.
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.highlight",
        "Highlight",
        env!("CARGO_PKG_VERSION"),
        "Colored highlight syntax with ==text== and =={color}text==",
        vec!["editor_extension".into()],
    )
    .ui(vec![serde_json::json!({
        "slot": "EditorExtension",
        "extension_id": "coloredHighlight",
        "node_type": "InlineMark",
        "markdown": {
            "level": "Inline",
            "open": "==",
            "close": "==",
            "attribute_syntax": {
                "attribute": "color",
                "open": "{",
                "close": "}",
                "position": "after_open"
            }
        },
        "html_tag": "mark",
        "base_css_class": "highlight-mark",
        "attributes": [{
            "name": "color",
            "default": "yellow",
            "html_attribute": "data-highlight-color",
            "valid_values": ["red", "orange", "yellow", "green", "cyan",
                             "blue", "violet", "pink", "brown", "grey"],
            "css_class_prefix": "highlight-"
        }],
        "css": HIGHLIGHT_CSS,
        "keyboard_shortcut": "Mod-Shift-h",
        "toolbar": {
            "icon": "highlighter",
            "label": "Highlight"
        },
        "render_export": null,
        "edit_mode": null,
        "click_behavior": null,
        "insert_command": null
    })])
    .min_app_version("1.4.0");

    Ok(serde_json::to_string(&manifest)?)
}

/// Handle commands dispatched by the host (none for this plugin).
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let request: CommandRequest = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let response = CommandResponse::err(format!("Unknown command: {}", request.command));

    Ok(serde_json::to_string(&response)?)
}

/// Handle lifecycle events (no-op for highlight plugin).
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
