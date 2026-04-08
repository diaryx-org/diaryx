//! Format conversion orchestration.
//!
//! Downloads pandoc/typst WASM binaries on demand and runs them via
//! `host::wasi::run`. All converter state is cached in plugin storage.

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

use diaryx_plugin_sdk::host;

// ============================================================================
// Constants
// ============================================================================

const PANDOC_WASM_URL: &str = "https://unpkg.com/wasm-pandoc@1.0.0/src/pandoc.wasm";
const PANDOC_STORAGE_KEY: &str = "converter:pandoc_wasm";
/// Lightweight sentinel key — avoids loading the full WASM blob just to check existence.
const PANDOC_READY_KEY: &str = "converter:pandoc_ready";

// ============================================================================
// Export format metadata
// ============================================================================

/// Metadata for a supported export format.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExportFormat {
    pub id: String,
    pub label: String,
    pub extension: String,
    pub requires_converter: bool,
}

/// All supported export formats.
pub fn get_export_formats() -> Vec<ExportFormat> {
    vec![
        ExportFormat {
            id: "markdown".into(),
            label: "Markdown".into(),
            extension: "md".into(),
            requires_converter: false,
        },
        ExportFormat {
            id: "html".into(),
            label: "HTML".into(),
            extension: "html".into(),
            requires_converter: false,
        },
        ExportFormat {
            id: "docx".into(),
            label: "Word (DOCX)".into(),
            extension: "docx".into(),
            requires_converter: true,
        },
        ExportFormat {
            id: "epub".into(),
            label: "EPUB".into(),
            extension: "epub".into(),
            requires_converter: true,
        },
        ExportFormat {
            id: "latex".into(),
            label: "LaTeX".into(),
            extension: "tex".into(),
            requires_converter: true,
        },
        ExportFormat {
            id: "odt".into(),
            label: "OpenDocument (ODT)".into(),
            extension: "odt".into(),
            requires_converter: true,
        },
        ExportFormat {
            id: "rst".into(),
            label: "reStructuredText".into(),
            extension: "rst".into(),
            requires_converter: true,
        },
        ExportFormat {
            id: "pdf".into(),
            label: "PDF".into(),
            extension: "pdf".into(),
            requires_converter: true,
        },
    ]
}

// ============================================================================
// Converter availability
// ============================================================================

/// Check if a converter is available in plugin storage.
///
/// Uses a lightweight sentinel key so this doesn't load the full WASM blob.
pub fn is_converter_available(name: &str) -> bool {
    let ready_key = match name {
        "pandoc" => PANDOC_READY_KEY,
        _ => return false,
    };
    host::storage::get(ready_key)
        .map(|v| v.is_some())
        .unwrap_or(false)
}

/// Download a converter WASM binary and store it in plugin storage.
pub fn download_converter(name: &str) -> Result<(), String> {
    let (url, key, ready_key) = match name {
        "pandoc" => (PANDOC_WASM_URL, PANDOC_STORAGE_KEY, PANDOC_READY_KEY),
        _ => return Err(format!("Unknown converter: {name}")),
    };

    host::log::log("info", &format!("Downloading {name} WASM from {url}..."));

    let response = host::http::request("GET", url, &HashMap::new(), None)?;

    if response.status != 200 {
        return Err(format!(
            "Failed to download {name}: HTTP {} — {}",
            response.status,
            response.body.chars().take(200).collect::<String>()
        ));
    }

    let wasm_bytes = if let Some(body_b64) = response.body_base64 {
        BASE64
            .decode(body_b64)
            .map_err(|e| format!("Failed to decode converter payload: {e}"))?
    } else {
        response.body.into_bytes()
    };
    host::storage::set(key, &wasm_bytes)?;
    // Write a lightweight sentinel so is_converter_available doesn't load the full blob.
    host::storage::set(ready_key, b"1")?;

    host::log::log(
        "info",
        &format!("Downloaded {name} WASM ({} bytes)", wasm_bytes.len()),
    );
    Ok(())
}

/// Ensure a converter is available, downloading it if needed.
///
/// Called during plugin init to pre-cache the converter WASM.
pub fn ensure_converter(name: &str) {
    if !is_converter_available(name) {
        if let Err(e) = download_converter(name) {
            host::log::log("warn", &format!("Failed to download {name} on init: {e}"));
        }
    }
}

// ============================================================================
// Format conversion
// ============================================================================

/// Convert content from one format to another using pandoc.
pub fn convert_format(
    content: &str,
    from: &str,
    to: &str,
    resources: Option<&HashMap<String, String>>,
) -> Result<ConvertResult, String> {
    // Ensure pandoc is available, downloading on demand if needed
    if !is_converter_available("pandoc") {
        download_converter("pandoc")?;
    }

    // Build pandoc arguments
    let mut args = vec![
        "-f".to_string(),
        from.to_string(),
        "-t".to_string(),
        to.to_string(),
        "--standalone".to_string(),
    ];

    // For binary output formats, use --output to write to a file
    let binary_formats = ["docx", "epub", "odt", "pdf"];
    let is_binary = binary_formats.contains(&to);
    let output_filename = if is_binary {
        let name = format!("output.{to}");
        args.push("-o".to_string());
        args.push(name.clone());
        Some(name)
    } else {
        None
    };

    // Prepare virtual filesystem files
    let mut files = HashMap::new();

    // Add resource files (already base64-encoded from the caller)
    if let Some(res) = resources {
        for (path, b64_content) in res {
            files.insert(path.clone(), b64_content.clone());
        }
    }

    // Stdin is the main content
    let stdin_b64 = BASE64.encode(content.as_bytes());

    let output_files = output_filename.as_ref().map(|f| vec![f.clone()]);

    let request = host::wasi::WasiRunRequest {
        module_key: PANDOC_STORAGE_KEY.to_string(),
        args,
        stdin: Some(stdin_b64),
        files: if files.is_empty() { None } else { Some(files) },
        output_files,
    };

    let result = host::wasi::run(&request)?;

    if result.exit_code != 0 {
        return Err(format!(
            "Pandoc exited with code {}: {}",
            result.exit_code, result.stderr
        ));
    }

    // For binary formats, the output is in the captured files
    if let Some(ref fname) = output_filename {
        if let Some(ref captured) = result.files {
            if let Some(b64_output) = captured.get(fname) {
                return Ok(ConvertResult {
                    content: None,
                    binary: Some(b64_output.clone()),
                    stderr: result.stderr,
                });
            }
        }
        return Err(format!(
            "Pandoc did not produce output file: {fname}\nstderr: {}",
            result.stderr
        ));
    }

    // For text formats, output is in stdout
    let stdout_bytes = BASE64
        .decode(&result.stdout)
        .map_err(|e| format!("Failed to decode stdout: {e}"))?;
    let text = String::from_utf8(stdout_bytes)
        .map_err(|e| format!("Pandoc output is not valid UTF-8: {e}"))?;

    Ok(ConvertResult {
        content: Some(text),
        binary: None,
        stderr: result.stderr,
    })
}

/// Result of a format conversion.
#[derive(Debug, Serialize)]
pub struct ConvertResult {
    /// Text output (for text formats like HTML, LaTeX, RST).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Binary output as base64 (for binary formats like DOCX, PDF).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
    /// Pandoc stderr output (warnings, etc.).
    pub stderr: String,
}
