//! Extism guest plugin for Diaryx import functionality.
//!
//! Provides parsing (Day One, Markdown) and orchestration (write_entries,
//! import_directory_in_place) as plugin commands.

mod directory;
mod host_bridge;
mod orchestrate;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use extism_pdk::*;
use serde_json::Value as JsonValue;

// ============================================================================
// Types
// ============================================================================

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
    #[serde(default)]
    cli: Vec<JsonValue>,
    #[serde(default)]
    permissions: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CommandRequest {
    command: String,
    params: JsonValue,
}

#[derive(serde::Serialize)]
struct CommandResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ============================================================================
// Plugin exports
// ============================================================================

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest {
        id: "diaryx.import".to_string(),
        name: "Import".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: "Import entries from Day One, Markdown directories, and other formats"
            .to_string(),
        capabilities: vec!["custom_commands".to_string()],
        ui: vec![],
        commands: vec![
            "ParseDayOne".to_string(),
            "ParseMarkdownFile".to_string(),
            "ImportEntries".to_string(),
            "ImportDirectoryInPlace".to_string(),
        ],
        cli: vec![serde_json::json!({
            "name": "import",
            "about": "Import entries from external formats",
            "aliases": ["imp"],
            "requires_workspace": true,
            "subcommands": [
                {
                    "name": "email",
                    "about": "Import emails from .eml files, directories of .eml files, or .mbox archives",
                    "native_handler": "import_email",
                    "requires_workspace": true,
                    "args": [
                        {
                            "name": "source",
                            "help": "Source: .eml file, directory of .eml files, or .mbox file",
                            "required": true,
                            "value_type": "Path"
                        },
                        {
                            "name": "folder",
                            "help": "Base folder for imported emails",
                            "short": "f",
                            "long": "folder",
                            "default_value": "emails",
                            "value_type": "String"
                        },
                        {
                            "name": "dry_run",
                            "help": "Show what would be done without writing",
                            "long": "dry-run",
                            "is_flag": true
                        },
                        {
                            "name": "verbose",
                            "help": "Print each email as it's processed",
                            "short": "v",
                            "long": "verbose",
                            "is_flag": true
                        }
                    ]
                },
                {
                    "name": "dayone",
                    "about": "Import entries from a Day One Journal.json export",
                    "native_handler": "import_dayone",
                    "requires_workspace": true,
                    "args": [
                        {
                            "name": "source",
                            "help": "Path to Journal.json or Day One ZIP export",
                            "required": true,
                            "value_type": "Path"
                        },
                        {
                            "name": "folder",
                            "help": "Base folder for imported entries",
                            "short": "f",
                            "long": "folder",
                            "default_value": "journal",
                            "value_type": "String"
                        },
                        {
                            "name": "dry_run",
                            "help": "Show what would be done without writing",
                            "long": "dry-run",
                            "is_flag": true
                        },
                        {
                            "name": "verbose",
                            "help": "Print each entry as it's processed",
                            "short": "v",
                            "long": "verbose",
                            "is_flag": true
                        }
                    ]
                },
                {
                    "name": "markdown",
                    "about": "Import a directory of markdown files (Obsidian, Logseq, Bear, iA Writer, etc.)",
                    "native_handler": "import_markdown",
                    "requires_workspace": true,
                    "args": [
                        {
                            "name": "source",
                            "help": "Path to directory of markdown files",
                            "required": true,
                            "value_type": "Path"
                        },
                        {
                            "name": "folder",
                            "help": "Base folder name in workspace (default: source directory name)",
                            "short": "f",
                            "long": "folder",
                            "value_type": "String"
                        },
                        {
                            "name": "dry_run",
                            "help": "Show what would be done without writing",
                            "long": "dry-run",
                            "is_flag": true
                        },
                        {
                            "name": "verbose",
                            "help": "Print each file as it's processed",
                            "short": "v",
                            "long": "verbose",
                            "is_flag": true
                        }
                    ]
                }
            ]
        })],
        permissions: vec![
            "read_files".to_string(),
            "edit_files".to_string(),
            "create_files".to_string(),
        ],
    };
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    host_bridge::log_message("info", "Import plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host_bridge::log_message("info", "Import plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;

    let response = match req.command.as_str() {
        "ParseDayOne" => handle_parse_dayone(req.params),
        "ParseMarkdownFile" => handle_parse_markdown(req.params),
        "ImportEntries" => handle_import_entries(req.params),
        "ImportDirectoryInPlace" => handle_import_directory_in_place(req.params),
        _ => CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Unknown command: {}", req.command)),
        },
    };

    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    Ok("{}".to_string())
}

#[plugin_fn]
pub fn set_config(_input: String) -> FnResult<String> {
    Ok(String::new())
}

// ============================================================================
// Command handlers
// ============================================================================

fn handle_parse_dayone(params: JsonValue) -> CommandResponse {
    let data_b64 = match params.get("data").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some("Missing 'data' parameter (base64-encoded bytes)".to_string()),
            };
        }
    };

    let bytes = match BASE64.decode(data_b64) {
        Ok(b) => b,
        Err(e) => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to decode base64: {e}")),
            };
        }
    };

    let result = diaryx_core::import::dayone::parse_dayone_auto(&bytes);

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for r in result.entries {
        match r {
            Ok(entry) => entries.push(entry),
            Err(e) => errors.push(e),
        }
    }

    #[derive(serde::Serialize)]
    struct ParseResult {
        entries: Vec<diaryx_core::import::ImportedEntry>,
        errors: Vec<String>,
        journal_name: Option<String>,
    }

    match serde_json::to_value(&ParseResult {
        entries,
        errors,
        journal_name: result.journal_name,
    }) {
        Ok(data) => CommandResponse {
            success: true,
            data: Some(data),
            error: None,
        },
        Err(e) => CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to serialize: {e}")),
        },
    }
}

fn handle_parse_markdown(params: JsonValue) -> CommandResponse {
    let data_b64 = match params.get("data").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some("Missing 'data' parameter (base64-encoded bytes)".to_string()),
            };
        }
    };

    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown.md");

    let bytes = match BASE64.decode(data_b64) {
        Ok(b) => b,
        Err(e) => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to decode base64: {e}")),
            };
        }
    };

    match diaryx_core::import::markdown::parse_markdown_file(&bytes, filename) {
        Ok(entry) => match serde_json::to_value(&entry) {
            Ok(data) => CommandResponse {
                success: true,
                data: Some(data),
                error: None,
            },
            Err(e) => CommandResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize: {e}")),
            },
        },
        Err(e) => CommandResponse {
            success: false,
            data: None,
            error: Some(e),
        },
    }
}

fn handle_import_entries(params: JsonValue) -> CommandResponse {
    let entries_json = match params.get("entries_json").and_then(|v| v.as_str()) {
        Some(j) => j,
        None => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some("Missing 'entries_json' parameter".to_string()),
            };
        }
    };

    let folder = match params.get("folder").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some("Missing 'folder' parameter".to_string()),
            };
        }
    };

    let parent_path = params.get("parent_path").and_then(|v| v.as_str());

    let entries: Vec<diaryx_core::import::ImportedEntry> = match serde_json::from_str(entries_json)
    {
        Ok(e) => e,
        Err(e) => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some(format!("Invalid entries JSON: {e}")),
            };
        }
    };

    let result = orchestrate::write_entries(folder, &entries, parent_path);

    match serde_json::to_value(&result) {
        Ok(data) => CommandResponse {
            success: true,
            data: Some(data),
            error: None,
        },
        Err(e) => CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Failed to serialize result: {e}")),
        },
    }
}

fn handle_import_directory_in_place(params: JsonValue) -> CommandResponse {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");

    match directory::import_directory_in_place(path) {
        Ok(result) => match serde_json::to_value(&result) {
            Ok(data) => CommandResponse {
                success: true,
                data: Some(data),
                error: None,
            },
            Err(e) => CommandResponse {
                success: false,
                data: None,
                error: Some(format!("Failed to serialize result: {e}")),
            },
        },
        Err(e) => CommandResponse {
            success: false,
            data: None,
            error: Some(e),
        },
    }
}
