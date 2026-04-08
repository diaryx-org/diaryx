//! Math rendering Extism guest plugin for Diaryx.
//!
//! Provides inline (`$...$`) and block (`$$...$$`) math rendering using
//! LaTeX syntax. Rendering is done entirely inside the WASM plugin via
//! `pulldown-latex`, which converts LaTeX to MathML for native browser display.

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;

// ============================================================================
// Render types
// ============================================================================

#[derive(serde::Deserialize)]
struct RenderInput {
    /// The math source text (without delimiters).
    source: String,
    /// Whether this is display mode (block) or inline.
    #[serde(default)]
    display_mode: bool,
}

#[derive(serde::Serialize)]
struct RenderOutput {
    /// Rendered HTML (MathML).
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    /// Error message if rendering failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ============================================================================
// LaTeX → MathML rendering
// ============================================================================

/// Render LaTeX source to MathML using pulldown-latex.
fn render_latex(source: &str, display_mode: bool) -> RenderOutput {
    use pulldown_latex::{
        Parser, Storage,
        config::{DisplayMode, RenderConfig},
        push_mathml,
    };

    let storage = Storage::new();
    let parser = Parser::new(source, &storage);
    let mut mathml = String::new();

    let mut config = RenderConfig {
        display_mode: if display_mode {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        ..RenderConfig::default()
    };
    config.annotation = Some(source);

    match push_mathml(&mut mathml, parser, config) {
        Ok(()) => RenderOutput {
            html: Some(mathml),
            error: None,
        },
        Err(e) => RenderOutput {
            html: None,
            error: Some(format!("{e}")),
        },
    }
}

// ============================================================================
// Guest exports
// ============================================================================

/// Return the plugin manifest.
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.math",
        "Math",
        env!("CARGO_PKG_VERSION"),
        "LaTeX math rendering with inline ($...$) and block ($$...$$) support",
        vec!["editor_extension".into()],
    )
    .ui(vec![
        // Inline math: $...$
        serde_json::json!({
            "slot": "EditorExtension",
            "extension_id": "mathInline",
            "node_type": "InlineAtom",
            "markdown": {
                "level": "Inline",
                "open": "$",
                "close": "$",
            },
            "render_export": "render_content",
            "edit_mode": "Popover",
            "css": null,
            "insert_command": {
                "label": "Math",
                "icon": "sigma",
                "description": "Insert inline math",
            },
        }),
        // Block math: $$...$$
        serde_json::json!({
            "slot": "EditorExtension",
            "extension_id": "mathBlock",
            "node_type": "BlockAtom",
            "markdown": {
                "level": "Block",
                "open": "$$",
                "close": "$$",
            },
            "render_export": "render_content",
            "edit_mode": "SourceToggle",
            "css": null,
            "insert_command": {
                "label": "Math Block",
                "icon": "square-sigma",
                "description": "Insert math block",
            },
        }),
    ]);

    Ok(serde_json::to_string(&manifest)?)
}

/// Render math content. Called by the host's generated TipTap extension node views.
///
/// Input: `{ "source": "E = mc^2", "display_mode": false }`
/// Output: `{ "html": "<math>...</math>" }` or `{ "error": "..." }`
#[plugin_fn]
pub fn render_content(input: String) -> FnResult<String> {
    let render_input: RenderInput = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let output = render_latex(&render_input.source, render_input.display_mode);

    Ok(serde_json::to_string(&output)?)
}

/// Handle commands dispatched by the host (none for this plugin).
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let request: CommandRequest = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let response = CommandResponse::err(format!("Unknown command: {}", request.command));

    Ok(serde_json::to_string(&response)?)
}

/// Handle lifecycle events (no-op for math plugin).
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
