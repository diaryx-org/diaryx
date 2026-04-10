//! Extism guest plugin for Diaryx import functionality.
//!
//! Provides parsing (Day One, Markdown, single `.eml`) and orchestration
//! (write_entries, import_directory_in_place) as plugin commands.

mod dayone;
mod directory;
#[cfg(feature = "email-import")]
mod email;
mod orchestrate;
mod types;

use diaryx_plugin_sdk::prelude::*;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use extism_pdk::*;
use serde_json::Value as JsonValue;

use types::{ImportResult, ImportedEntry};

#[cfg(not(feature = "markdown-import"))]
const MARKDOWN_IMPORT_DISABLED_ERROR: &str =
    "ParseMarkdownFile is unavailable: plugin built without `markdown-import` feature";

#[cfg(not(feature = "email-import"))]
const EMAIL_IMPORT_DISABLED_ERROR: &str =
    "ParseEml is unavailable: plugin built without `email-import` feature";

// ============================================================================
// Types
// ============================================================================

const IMPORT_CONFIG_STORAGE_KEY: &str = "import.settings.config";

#[derive(serde::Serialize, serde::Deserialize)]
struct ImportSettingsConfig {
    #[serde(default = "default_import_format")]
    import_format: String,
    #[serde(default = "default_dayone_folder")]
    dayone_folder: String,
    #[serde(default)]
    dayone_parent_path: String,
    #[serde(default = "default_markdown_destination")]
    markdown_destination: String,
    #[serde(default = "default_markdown_subfolder")]
    markdown_subfolder: String,
}

impl Default for ImportSettingsConfig {
    fn default() -> Self {
        Self {
            import_format: default_import_format(),
            dayone_folder: default_dayone_folder(),
            dayone_parent_path: String::new(),
            markdown_destination: default_markdown_destination(),
            markdown_subfolder: default_markdown_subfolder(),
        }
    }
}

fn default_import_format() -> String {
    "dayone".to_string()
}

fn default_dayone_folder() -> String {
    "journal".to_string()
}

fn default_markdown_destination() -> String {
    "subfolder".to_string()
}

fn default_markdown_subfolder() -> String {
    "imported".to_string()
}

fn load_import_config() -> ImportSettingsConfig {
    match host::storage::get(IMPORT_CONFIG_STORAGE_KEY) {
        Ok(Some(bytes)) => {
            serde_json::from_slice::<ImportSettingsConfig>(&bytes).unwrap_or_default()
        }
        _ => ImportSettingsConfig::default(),
    }
}

fn save_import_config(config: &ImportSettingsConfig) {
    if let Ok(bytes) = serde_json::to_vec(config) {
        let _ = host::storage::set(IMPORT_CONFIG_STORAGE_KEY, &bytes);
    }
}

fn apply_import_config(config: &mut ImportSettingsConfig, incoming: &JsonValue) {
    if let Some(value) = incoming.get("import_format").and_then(|v| v.as_str()) {
        config.import_format = match value {
            "markdown" => "markdown".to_string(),
            _ => default_import_format(),
        };
    }
    if let Some(value) = incoming.get("dayone_folder").and_then(|v| v.as_str()) {
        config.dayone_folder = value.to_string();
    }
    if let Some(value) = incoming.get("dayone_parent_path").and_then(|v| v.as_str()) {
        config.dayone_parent_path = value.to_string();
    }
    if let Some(value) = incoming
        .get("markdown_destination")
        .and_then(|v| v.as_str())
    {
        config.markdown_destination = match value {
            "root" => "root".to_string(),
            _ => default_markdown_destination(),
        };
    }
    if let Some(value) = incoming.get("markdown_subfolder").and_then(|v| v.as_str()) {
        config.markdown_subfolder = value.to_string();
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ============================================================================
// Plugin exports
// ============================================================================

fn build_manifest() -> GuestManifest {
    #[allow(unused_mut)]
    let mut commands = vec![
        "ChooseDayOneParent".to_string(),
        "SaveDayOneParentSelection".to_string(),
        "ResetDayOneParent".to_string(),
        "StartDayOneImport".to_string(),
        "ParseDayOne".to_string(),
        "ImportDayOne".to_string(),
        "StartMarkdownFolderImport".to_string(),
        "StartMarkdownZipImport".to_string(),
        "PrepareMarkdownImport".to_string(),
        "FinalizeMarkdownImport".to_string(),
        "ImportEntries".to_string(),
        "ImportDirectoryInPlace".to_string(),
    ];

    #[cfg(feature = "markdown-import")]
    commands.push("ParseMarkdownFile".to_string());

    #[cfg(feature = "email-import")]
    commands.push("ParseEml".to_string());

    #[allow(unused_mut)]
    let mut subcommands = vec![
        serde_json::json!({
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
        }),
        serde_json::json!({
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
        }),
    ];

    #[cfg(feature = "markdown-import")]
    subcommands.push(serde_json::json!({
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
    }));

    GuestManifest::new(
        "diaryx.import".to_string(),
        "Import".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
        "Import entries from Day One, Markdown directories, and other formats"
            .to_string(),
        vec!["custom_commands".to_string()],
    )
    .ui(vec![serde_json::json!({
        "slot": "SettingsTab",
        "id": "import-settings",
        "label": "Import",
        "icon": serde_json::Value::Null,
        "fields": [
            {
                "type": "Section",
                "label": "Format",
                "description": "Choose which kind of content you want to import."
            },
            {
                "type": "Select",
                "key": "import_format",
                "label": "Import source",
                "description": "Show the controls for the import flow you want.",
                "options": [
                    { "value": "dayone", "label": "Day One" },
                    { "value": "markdown", "label": "Markdown" }
                ]
            },
            {
                "type": "Conditional",
                "condition": "config:import_format=dayone",
                "fields": [
                    {
                        "type": "Section",
                        "label": "Day One",
                        "description": "Import a Day One JSON or ZIP export into Diaryx entries. Imports go to the workspace root by default unless you choose a parent entry."
                    },
                    {
                        "type": "Text",
                        "key": "dayone_folder",
                        "label": "Import folder",
                        "description": "Base folder for imported Day One entries.",
                        "placeholder": "journal"
                    },
                    {
                        "type": "Button",
                        "label": "Choose Parent Entry...",
                        "command": "ChooseDayOneParent",
                        "variant": "outline"
                    },
                    {
                        "type": "Button",
                        "label": "Use Workspace Root",
                        "command": "ResetDayOneParent",
                        "variant": "outline"
                    },
                    {
                        "type": "Button",
                        "label": "Select Day One Export...",
                        "command": "StartDayOneImport",
                        "variant": "outline"
                    }
                ]
            },
            {
                "type": "Conditional",
                "condition": "config:import_format=markdown",
                "fields": [
                    {
                        "type": "Section",
                        "label": "Markdown",
                        "description": "Import a markdown folder or ZIP, then build Diaryx hierarchy metadata."
                    },
                    {
                        "type": "Select",
                        "key": "markdown_destination",
                        "label": "Destination",
                        "description": "Import into the workspace root or into a subfolder.",
                        "options": [
                            { "value": "subfolder", "label": "Subfolder" },
                            { "value": "root", "label": "Workspace root" }
                        ]
                    },
                    {
                        "type": "Conditional",
                        "condition": "config:markdown_destination=subfolder",
                        "fields": [
                            {
                                "type": "Text",
                                "key": "markdown_subfolder",
                                "label": "Folder name",
                                "description": "Used when destination is set to subfolder.",
                                "placeholder": "imported"
                            }
                        ]
                    },
                    {
                        "type": "Button",
                        "label": "Select Folder...",
                        "command": "StartMarkdownFolderImport",
                        "variant": "outline"
                    },
                    {
                        "type": "Button",
                        "label": "Select ZIP...",
                        "command": "StartMarkdownZipImport",
                        "variant": "outline"
                    }
                ]
            }
        ],
        "component": serde_json::Value::Null
    })])
    .commands(commands)
    .cli(vec![serde_json::json!({
        "name": "import",
        "about": "Import entries from external formats",
        "aliases": ["imp"],
        "requires_workspace": true,
        "subcommands": subcommands
    })])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] }
        }),
        reasons: std::collections::HashMap::from([
            ("read_files".to_string(), "Read existing entries during import.".to_string()),
            ("edit_files".to_string(), "Update entry metadata during import.".to_string()),
            ("create_files".to_string(), "Create new entries from imported data.".to_string()),
        ]),
    })
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&build_manifest())?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    host::log::log("info", "Import plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host::log::log("info", "Import plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    Ok(serde_json::to_string(&dispatch_command(req))?)
}

#[plugin_fn]
pub fn execute_typed_command(input: String) -> FnResult<String> {
    execute_typed_command_inner(&input).map_err(|error| extism_pdk::Error::msg(error).into())
}

fn execute_typed_command_inner(input: &str) -> Result<String, String> {
    let parsed: JsonValue =
        serde_json::from_str(&input).map_err(|e| format!("Invalid JSON: {e}"))?;

    let cmd_type = parsed
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing `type` in typed command".to_string())?;

    let params = parsed.get("params").cloned().unwrap_or(JsonValue::Null);

    match dispatch_typed_command(cmd_type, params) {
        Ok(Some(data)) => {
            let response = serde_json::json!({
                "type": "PluginResult",
                "data": data,
            });
            serde_json::to_string(&response).map_err(|e| format!("Serialize error: {e}"))
        }
        Ok(None) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    Ok(serde_json::to_string(&load_import_config())?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let mut config = load_import_config();
    let incoming: JsonValue = serde_json::from_str(&input)?;
    apply_import_config(&mut config, &incoming);
    save_import_config(&config);
    Ok(String::new())
}

// ============================================================================
// Command handlers
// ============================================================================

fn success_response(data: JsonValue) -> CommandResponse {
    CommandResponse::ok(data)
}

fn failure_response(message: impl Into<String>) -> CommandResponse {
    CommandResponse::err(message)
}

fn handle_choose_dayone_parent(_params: JsonValue) -> CommandResponse {
    success_response(serde_json::json!({
        "host_action": {
            "type": "pick-workspace-entry",
            "payload": {
                "title": "Choose Parent Entry",
                "description": "Choose the entry to import Day One content under, or cancel to keep the workspace root.",
                "placeholder": "Search entries...",
                "allow_root": true
            }
        },
        "follow_up": {
            "command": "SaveDayOneParentSelection"
        }
    }))
}

fn handle_save_dayone_parent_selection(params: JsonValue) -> CommandResponse {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let selected_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("selected entry");
    let message = if path.trim().is_empty() {
        "Day One import will use the workspace root"
    } else {
        return success_response(serde_json::json!({
            "message": format!("Day One import will go under {selected_name}"),
            "config_patch": {
                "dayone_parent_path": path.trim()
            }
        }));
    };

    success_response(serde_json::json!({
        "message": message,
        "config_patch": {
            "dayone_parent_path": path.trim()
        }
    }))
}

fn handle_reset_dayone_parent(_params: JsonValue) -> CommandResponse {
    success_response(serde_json::json!({
        "message": "Day One import will use the workspace root",
        "config_patch": {
            "dayone_parent_path": ""
        }
    }))
}

fn handle_start_dayone_import(_params: JsonValue) -> CommandResponse {
    let config = load_import_config();
    let folder = non_empty_or(&config.dayone_folder, "journal");
    let parent_path = optional_trimmed(&config.dayone_parent_path);

    success_response(serde_json::json!({
        "host_action": {
            "type": "pick-local-file",
            "payload": {
                "accept": ".json,.zip"
            }
        },
        "follow_up": {
            "command": "ImportDayOne",
            "params": {
                "folder": folder,
                "parent_path": parent_path
            }
        }
    }))
}

fn handle_start_markdown_folder_import(_params: JsonValue) -> CommandResponse {
    success_response(serde_json::json!({
        "host_action": {
            "type": "pick-local-directory"
        },
        "follow_up": {
            "command": "PrepareMarkdownImport"
        }
    }))
}

fn handle_start_markdown_zip_import(_params: JsonValue) -> CommandResponse {
    success_response(serde_json::json!({
        "host_action": {
            "type": "pick-local-file",
            "payload": {
                "accept": ".zip"
            }
        },
        "follow_up": {
            "command": "PrepareMarkdownImport"
        }
    }))
}

fn handle_prepare_markdown_import(params: JsonValue) -> CommandResponse {
    let selection_token = match params.get("selection_token").and_then(|v| v.as_str()) {
        Some(value) if !value.trim().is_empty() => value,
        _ => return failure_response("Missing selected local source for markdown import"),
    };
    let config = load_import_config();
    let destination_prefix = if config.markdown_destination == "root" {
        String::new()
    } else {
        non_empty_or(&config.markdown_subfolder, "imported")
    };

    success_response(serde_json::json!({
        "host_action": {
            "type": "import-local-selection-to-workspace",
            "payload": {
                "selection_token": selection_token,
                "destination_prefix": destination_prefix
            }
        },
        "follow_up": {
            "command": "FinalizeMarkdownImport"
        }
    }))
}

fn handle_finalize_markdown_import(params: JsonValue) -> CommandResponse {
    let imported = params
        .get("files_imported")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let skipped = params
        .get("files_skipped")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let mut errors: Vec<String> = params
        .get("errors")
        .and_then(|v| v.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if imported == 0 {
        return failure_response(
            errors
                .first()
                .cloned()
                .unwrap_or_else(|| "No files found to import".to_string()),
        );
    }

    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    match directory::import_directory_in_place(path) {
        Ok(result) => {
            errors.extend(result.errors);
            success_response(serde_json::json!({
                "imported": imported,
                "skipped": skipped + result.skipped,
                "errors": errors,
                "attachment_count": result.attachment_count,
                "message": format!("Imported {} files", imported)
            }))
        }
        Err(error) => failure_response(error),
    }
}

fn handle_parse_dayone(params: JsonValue) -> CommandResponse {
    let bytes = match resolve_input_bytes(&params) {
        Ok(b) => b,
        Err(error) => {
            return CommandResponse::err(error);
        }
    };

    let parsed = parse_dayone_entries(&bytes);

    #[derive(serde::Serialize)]
    struct ParseResult {
        entries: Vec<ImportedEntry>,
        errors: Vec<String>,
        journal_name: Option<String>,
    }

    match serde_json::to_value(&ParseResult {
        entries: parsed.entries,
        errors: parsed.errors,
        journal_name: parsed.journal_name,
    }) {
        Ok(data) => CommandResponse::ok(data),
        Err(e) => CommandResponse::err(format!("Failed to serialize: {e}")),
    }
}

fn handle_import_dayone(params: JsonValue) -> CommandResponse {
    let bytes = match resolve_input_bytes(&params) {
        Ok(b) => b,
        Err(error) => {
            return CommandResponse::err(error);
        }
    };

    let folder = match params.get("folder").and_then(|v| v.as_str()) {
        Some(folder) => folder,
        None => {
            return CommandResponse::err("Missing 'folder' parameter");
        }
    };

    let parent_path = params.get("parent_path").and_then(|v| v.as_str());
    let result = import_dayone_direct(&bytes, folder, parent_path);

    match serde_json::to_value(&result) {
        Ok(data) => CommandResponse::ok(data),
        Err(e) => CommandResponse::err(format!("Failed to serialize result: {e}")),
    }
}

fn import_dayone_direct(bytes: &[u8], folder: &str, parent_path: Option<&str>) -> ImportResult {
    let mut writer = orchestrate::ImportWriter::new(folder, parent_path);

    let mut stream = match dayone::stream_dayone_auto(bytes) {
        Ok(stream) => stream,
        Err(error) => {
            return ImportResult {
                imported: 0,
                skipped: 1,
                errors: vec![error],
                attachment_count: 0,
            };
        }
    };

    let mut parse_errors = Vec::new();
    while let Some(entry_result) = stream.next_entry() {
        match entry_result {
            Ok(entry) => writer.write_entry(&entry),
            Err(error) => parse_errors.push(error),
        }
    }

    let mut result = writer.finish();
    result.skipped += parse_errors.len();
    result.errors.extend(parse_errors);
    result
}

struct ParsedDayOneEntries {
    entries: Vec<ImportedEntry>,
    errors: Vec<String>,
    journal_name: Option<String>,
}

fn parse_dayone_entries(bytes: &[u8]) -> ParsedDayOneEntries {
    let result = dayone::parse_dayone_auto(bytes);

    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for r in result.entries {
        match r {
            Ok(entry) => entries.push(entry),
            Err(e) => errors.push(e),
        }
    }

    ParsedDayOneEntries {
        entries,
        errors,
        journal_name: result.journal_name,
    }
}

fn resolve_input_bytes(params: &JsonValue) -> Result<Vec<u8>, String> {
    if let Some(file_key) = params.get("file_key").and_then(|v| v.as_str()) {
        let bytes = host::files::request(file_key)?;
        if bytes.is_empty() {
            return Err(format!("Requested file not available for key '{file_key}'"));
        }
        return Ok(bytes);
    }

    let data_b64 = params.get("data").and_then(|v| v.as_str()).ok_or_else(|| {
        "Missing 'data' parameter (base64-encoded bytes) or 'file_key' parameter".to_string()
    })?;

    BASE64
        .decode(data_b64)
        .map_err(|e| format!("Failed to decode base64: {e}"))
}

#[cfg(not(feature = "markdown-import"))]
fn markdown_import_disabled_response() -> CommandResponse {
    CommandResponse::err(MARKDOWN_IMPORT_DISABLED_ERROR)
}

#[cfg(not(feature = "email-import"))]
fn email_import_disabled_response() -> CommandResponse {
    CommandResponse::err(EMAIL_IMPORT_DISABLED_ERROR)
}

#[cfg(feature = "email-import")]
fn handle_parse_eml(params: JsonValue) -> CommandResponse {
    let bytes = match resolve_input_bytes(&params) {
        Ok(b) => b,
        Err(error) => return CommandResponse::err(error),
    };

    match email::parse_eml(&bytes) {
        Ok(entry) => match serde_json::to_value(&entry) {
            Ok(data) => CommandResponse::ok(data),
            Err(e) => CommandResponse::err(format!("Failed to serialize: {e}")),
        },
        Err(e) => CommandResponse::err(e),
    }
}

#[cfg(feature = "markdown-import")]
fn handle_parse_markdown(params: JsonValue) -> CommandResponse {
    let data_b64 = match params.get("data").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => {
            return CommandResponse::err("Missing 'data' parameter (base64-encoded bytes)");
        }
    };

    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown.md");

    let bytes = match BASE64.decode(data_b64) {
        Ok(b) => b,
        Err(e) => {
            return CommandResponse::err(format!("Failed to decode base64: {e}"));
        }
    };

    match parse_markdown_file(&bytes, filename) {
        Ok(entry) => match serde_json::to_value(&entry) {
            Ok(data) => CommandResponse::ok(data),
            Err(e) => CommandResponse::err(format!("Failed to serialize: {e}")),
        },
        Err(e) => CommandResponse::err(e),
    }
}

fn dispatch_command(req: CommandRequest) -> CommandResponse {
    match req.command.as_str() {
        "ChooseDayOneParent" => handle_choose_dayone_parent(req.params),
        "SaveDayOneParentSelection" => handle_save_dayone_parent_selection(req.params),
        "ResetDayOneParent" => handle_reset_dayone_parent(req.params),
        "StartDayOneImport" => handle_start_dayone_import(req.params),
        "ParseDayOne" => handle_parse_dayone(req.params),
        "ImportDayOne" => handle_import_dayone(req.params),
        "StartMarkdownFolderImport" => handle_start_markdown_folder_import(req.params),
        "StartMarkdownZipImport" => handle_start_markdown_zip_import(req.params),
        "PrepareMarkdownImport" => handle_prepare_markdown_import(req.params),
        "FinalizeMarkdownImport" => handle_finalize_markdown_import(req.params),
        #[cfg(feature = "markdown-import")]
        "ParseMarkdownFile" => handle_parse_markdown(req.params),
        #[cfg(not(feature = "markdown-import"))]
        "ParseMarkdownFile" => markdown_import_disabled_response(),
        #[cfg(feature = "email-import")]
        "ParseEml" => handle_parse_eml(req.params),
        #[cfg(not(feature = "email-import"))]
        "ParseEml" => email_import_disabled_response(),
        "ImportEntries" => handle_import_entries(req.params),
        "ImportDirectoryInPlace" => handle_import_directory_in_place(req.params),
        _ => CommandResponse::err(format!("Unknown command: {}", req.command)),
    }
}

fn dispatch_typed_command(command: &str, params: JsonValue) -> Result<Option<JsonValue>, String> {
    let response = dispatch_command(CommandRequest {
        command: command.to_string(),
        params,
    });

    if response.success {
        return Ok(Some(response.data.unwrap_or(JsonValue::Null)));
    }

    let error = response
        .error
        .unwrap_or_else(|| format!("Command failed: {command}"));
    if error == format!("Unknown command: {command}") {
        return Ok(None);
    }

    Err(error)
}

#[cfg(feature = "markdown-import")]
fn parse_markdown_file(bytes: &[u8], filename: &str) -> Result<ImportedEntry, String> {
    let content =
        std::str::from_utf8(bytes).map_err(|e| format!("Failed to decode markdown: {e}"))?;
    let parsed = diaryx_core::frontmatter::parse_or_empty(content).map_err(|e| e.to_string())?;

    let title = parsed
        .frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            std::path::Path::new(filename)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Untitled".to_string());

    Ok(ImportedEntry {
        title,
        date: None,
        body: parsed.body,
        metadata: parsed.frontmatter,
        attachments: Vec::new(),
    })
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static TEST_REQUESTED_FILES: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
        static TEST_STORAGE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
        static TEST_WRITTEN_TEXT_FILES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
        static TEST_WRITTEN_BINARY_FILES: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    }

    pub fn log_message(_level: &str, _message: &str) {}

    pub fn read_file(path: &str) -> Result<String, String> {
        TEST_WRITTEN_TEXT_FILES.with(|files| {
            files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| format!("No test text file at path: {path}"))
        })
    }

    pub fn list_files(prefix: &str) -> Result<Vec<String>, String> {
        let mut paths: Vec<String> = TEST_WRITTEN_TEXT_FILES.with(|files| {
            files
                .borrow()
                .keys()
                .filter(|path| path.starts_with(prefix))
                .cloned()
                .collect()
        });
        let mut binary_paths: Vec<String> = TEST_WRITTEN_BINARY_FILES.with(|files| {
            files
                .borrow()
                .keys()
                .filter(|path| path.starts_with(prefix))
                .cloned()
                .collect()
        });
        paths.append(&mut binary_paths);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn file_exists(path: &str) -> Result<bool, String> {
        let exists = TEST_WRITTEN_TEXT_FILES.with(|files| files.borrow().contains_key(path))
            || TEST_WRITTEN_BINARY_FILES.with(|files| files.borrow().contains_key(path));
        Ok(exists)
    }

    pub fn write_file(path: &str, content: &str) -> Result<(), String> {
        TEST_WRITTEN_TEXT_FILES.with(|files| {
            files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
        });
        Ok(())
    }

    pub fn write_binary(path: &str, content: &[u8]) -> Result<(), String> {
        TEST_WRITTEN_BINARY_FILES.with(|files| {
            files
                .borrow_mut()
                .insert(path.to_string(), content.to_vec());
        });
        Ok(())
    }

    pub fn request_file(key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(TEST_REQUESTED_FILES.with(|files| files.borrow().get(key).cloned()))
    }

    pub fn storage_get(key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(TEST_STORAGE.with(|storage| storage.borrow().get(key).cloned()))
    }

    pub fn storage_set(key: &str, data: &[u8]) -> Result<(), String> {
        TEST_STORAGE.with(|storage| {
            storage.borrow_mut().insert(key.to_string(), data.to_vec());
        });
        Ok(())
    }

    pub fn set_test_requested_file(key: &str, bytes: &[u8]) {
        TEST_REQUESTED_FILES.with(|files| {
            files.borrow_mut().insert(key.to_string(), bytes.to_vec());
        });
    }

    pub fn clear_test_requested_files() {
        TEST_REQUESTED_FILES.with(|files| files.borrow_mut().clear());
    }

    pub fn clear_test_storage() {
        TEST_STORAGE.with(|storage| storage.borrow_mut().clear());
    }

    pub fn clear_test_written_files() {
        TEST_WRITTEN_TEXT_FILES.with(|files| files.borrow_mut().clear());
        TEST_WRITTEN_BINARY_FILES.with(|files| files.borrow_mut().clear());
    }

    pub fn get_test_written_text_file(path: &str) -> Option<String> {
        TEST_WRITTEN_TEXT_FILES.with(|files| files.borrow().get(path).cloned())
    }

    pub fn get_test_written_binary_file(path: &str) -> Option<Vec<u8>> {
        TEST_WRITTEN_BINARY_FILES.with(|files| files.borrow().get(path).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed_manifest() -> GuestManifest {
        build_manifest()
    }

    #[test]
    fn manifest_declares_requested_permissions() {
        let parsed = parsed_manifest();
        let perms = parsed
            .requested_permissions
            .as_ref()
            .expect("import manifest should declare requested permissions");

        let defaults = &perms.defaults;
        assert_eq!(
            defaults
                .get("read_files")
                .and_then(|rule| rule.get("include"))
                .and_then(|include| include.as_array())
                .and_then(|include| include.first())
                .and_then(|value| value.as_str()),
            Some("all")
        );
        assert!(defaults.get("edit_files").is_some());
        assert!(defaults.get("create_files").is_some());
    }

    #[test]
    fn manifest_declares_import_settings_tab() {
        let parsed = parsed_manifest();
        let tab = parsed
            .ui
            .iter()
            .find(|ui| {
                ui.get("slot").and_then(|v| v.as_str()) == Some("SettingsTab")
                    && ui.get("id").and_then(|v| v.as_str()) == Some("import-settings")
            })
            .expect("import settings tab should exist");

        assert_eq!(tab.get("label").and_then(|v| v.as_str()), Some("Import"));
        assert!(tab.get("component").is_some_and(|v| v.is_null()));
        let fields = tab
            .get("fields")
            .and_then(|v| v.as_array())
            .expect("import settings tab should declare fields");
        assert!(fields.iter().any(|field| {
            field.get("type").and_then(|v| v.as_str()) == Some("Select")
                && field.get("key").and_then(|v| v.as_str()) == Some("import_format")
        }));
        assert!(fields.iter().any(|field| {
            field.get("type").and_then(|v| v.as_str()) == Some("Conditional")
                && field.get("condition").and_then(|v| v.as_str())
                    == Some("config:import_format=dayone")
        }));
    }

    #[test]
    fn get_config_returns_defaults() {
        test_helpers::clear_test_storage();

        let config = load_import_config();

        assert_eq!(config.import_format, "dayone");
        assert_eq!(config.dayone_folder, "journal");
        assert_eq!(config.markdown_destination, "subfolder");
        assert_eq!(config.markdown_subfolder, "imported");
    }

    #[test]
    fn set_config_persists_import_settings() {
        test_helpers::clear_test_storage();

        let mut config = load_import_config();
        apply_import_config(
            &mut config,
            &serde_json::json!({
                "import_format": "markdown",
                "dayone_folder": "travel",
                "markdown_destination": "root",
                "markdown_subfolder": "notes"
            }),
        );
        save_import_config(&config);

        let config = load_import_config();
        assert_eq!(config.import_format, "markdown");
        assert_eq!(config.dayone_folder, "travel");
        assert_eq!(config.markdown_destination, "root");
        assert_eq!(config.markdown_subfolder, "notes");
    }

    #[test]
    fn start_dayone_import_returns_file_picker_follow_up() {
        test_helpers::clear_test_storage();
        save_import_config(&ImportSettingsConfig {
            import_format: default_import_format(),
            dayone_folder: "journal".to_string(),
            dayone_parent_path: "Trips/Index.md".to_string(),
            markdown_destination: default_markdown_destination(),
            markdown_subfolder: default_markdown_subfolder(),
        });

        let response = dispatch_command(CommandRequest {
            command: "StartDayOneImport".to_string(),
            params: JsonValue::Null,
        });

        assert!(response.success);
        let data = response.data.expect("host action payload should exist");
        assert_eq!(
            data.get("host_action")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("pick-local-file")
        );
        assert_eq!(
            data.get("follow_up")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("ImportDayOne")
        );
    }

    #[test]
    fn finalize_markdown_import_combines_workspace_results() {
        test_helpers::clear_test_written_files();
        test_helpers::write_file("imported/index.md", "---\ntitle: Imported\n---\n")
            .expect("seed imported file");

        let response = dispatch_command(CommandRequest {
            command: "FinalizeMarkdownImport".to_string(),
            params: serde_json::json!({
                "path": "imported",
                "files_imported": 1,
                "files_skipped": 0,
                "errors": []
            }),
        });

        assert!(response.success);
        let data = response
            .data
            .expect("finalize response should include data");
        assert_eq!(data.get("imported").and_then(|v| v.as_u64()), Some(1));
    }

    #[test]
    #[cfg(feature = "markdown-import")]
    fn manifest_includes_markdown_when_enabled() {
        let parsed = parsed_manifest();
        assert!(parsed.commands.iter().any(|cmd| cmd == "ParseMarkdownFile"));

        let cli = parsed
            .cli
            .first()
            .expect("import CLI declaration should exist");
        let subcommands = cli
            .get("subcommands")
            .and_then(|v| v.as_array())
            .expect("subcommands should be an array");
        assert!(
            subcommands.iter().any(|cmd| {
                cmd.get("name")
                    .and_then(|name| name.as_str())
                    .is_some_and(|name| name == "markdown")
            }),
            "markdown CLI subcommand should be exposed when feature is enabled"
        );
    }

    #[test]
    #[cfg(not(feature = "markdown-import"))]
    fn manifest_omits_markdown_when_disabled() {
        let parsed = parsed_manifest();
        assert!(!parsed.commands.iter().any(|cmd| cmd == "ParseMarkdownFile"));

        let cli = parsed
            .cli
            .first()
            .expect("import CLI declaration should exist");
        let subcommands = cli
            .get("subcommands")
            .and_then(|v| v.as_array())
            .expect("subcommands should be an array");
        assert!(
            !subcommands.iter().any(|cmd| {
                cmd.get("name")
                    .and_then(|name| name.as_str())
                    .is_some_and(|name| name == "markdown")
            }),
            "markdown CLI subcommand should not be exposed when feature is disabled"
        );
    }

    #[test]
    #[cfg(not(feature = "markdown-import"))]
    fn parse_markdown_returns_feature_gate_error_when_disabled() {
        let response = dispatch_command(CommandRequest {
            command: "ParseMarkdownFile".to_string(),
            params: serde_json::json!({}),
        });

        assert!(!response.success);
        assert_eq!(response.data, None);
        assert_eq!(
            response.error.as_deref(),
            Some(MARKDOWN_IMPORT_DISABLED_ERROR)
        );
    }

    #[test]
    fn parse_dayone_can_read_requested_file_from_host() {
        let dayone_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "ABC123",
                "text": "# Host File Import\nThis came from host_request_file.",
                "creationDate": "2020-09-24T01:36:35Z",
                "starred": false,
                "tags": [],
                "isPinned": false
            }]
        }"##;

        test_helpers::clear_test_requested_files();
        test_helpers::set_test_requested_file("dayone_export", dayone_json.as_bytes());

        let response = dispatch_command(CommandRequest {
            command: "ParseDayOne".to_string(),
            params: serde_json::json!({ "file_key": "dayone_export" }),
        });

        test_helpers::clear_test_requested_files();

        assert!(response.success, "expected successful ParseDayOne response");
        let entries = response
            .data
            .as_ref()
            .and_then(|v| v.get("entries"))
            .and_then(|v| v.as_array())
            .expect("response should include entries array");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("title").and_then(|v| v.as_str()),
            Some("Host File Import")
        );
    }

    #[test]
    fn import_dayone_writes_entries_without_entries_json_roundtrip() {
        let dayone_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "ABC123",
                "text": "# Direct Import\nWritten in one plugin command.",
                "creationDate": "2020-09-24T01:36:35Z",
                "starred": false,
                "tags": [],
                "isPinned": false
            }]
        }"##;

        test_helpers::clear_test_requested_files();
        test_helpers::clear_test_written_files();
        test_helpers::set_test_requested_file("dayone_export", dayone_json.as_bytes());

        let response = dispatch_command(CommandRequest {
            command: "ImportDayOne".to_string(),
            params: serde_json::json!({
                "file_key": "dayone_export",
                "folder": "journal",
                "parent_path": null,
            }),
        });

        test_helpers::clear_test_requested_files();

        assert!(
            response.success,
            "expected successful ImportDayOne response"
        );
        let data = response
            .data
            .as_ref()
            .expect("import response should include data");
        assert_eq!(data.get("imported").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(data.get("skipped").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(
            test_helpers::get_test_written_text_file("journal/2020/09/2020-09-24-direct-import.md")
                .as_deref()
                .map(|content| content.contains("Written in one plugin command.")),
            Some(true)
        );

        test_helpers::clear_test_written_files();
    }

    #[test]
    fn import_writer_formats_attachments_and_body_as_markdown_root_links() {
        let root_index = "---\ntitle: Home\nlink_format: markdown_root\ncontents: []\n---\n";

        test_helpers::clear_test_written_files();
        test_helpers::write_file("index.md", root_index).expect("seed root index");

        let entry: ImportedEntry = serde_json::from_value(serde_json::json!({
            "title": "Gallery Entry",
            "date": "2020-09-24T01:36:35Z",
            "body": "Before\n![](_attachments/pic 1.jpg)\nAfter",
            "metadata": {},
            "attachments": [{
                "filename": "pic 1.jpg",
                "content_type": "image/jpeg",
                "data": [106, 112, 101, 103, 45, 98, 121, 116, 101, 115]
            }]
        }))
        .expect("valid imported entry fixture");

        let result = orchestrate::write_entries("imports", &[entry], None);
        assert_eq!(result.imported, 1);

        let entry_path = "imports/2020/09/2020-09-24-gallery-entry.md";
        let content =
            test_helpers::get_test_written_text_file(entry_path).expect("entry file written");
        assert!(content.contains("link:"), "entry should have a self-link");
        assert!(content.contains("part_of: '[2020-09](/imports/2020/09/2020_09.md)'"));
        // Attachment frontmatter links point to note files, not binaries.
        assert!(content.contains(
            "[pic 1.jpg](</imports/2020/09/2020-09-24-gallery-entry/_attachments/pic 1.jpg.md>)"
        ));
        // Body image references still point to binary.
        assert!(
            content.contains(
                "![](</imports/2020/09/2020-09-24-gallery-entry/_attachments/pic 1.jpg>)"
            )
        );
        assert_eq!(
            test_helpers::get_test_written_binary_file(
                "imports/2020/09/2020-09-24-gallery-entry/_attachments/pic 1.jpg"
            ),
            Some(b"jpeg-bytes".to_vec())
        );

        // Attachment note was created alongside binary.
        let note = test_helpers::get_test_written_text_file(
            "imports/2020/09/2020-09-24-gallery-entry/_attachments/pic 1.jpg.md",
        )
        .expect("attachment note should be written");
        assert!(note.contains("title: pic 1.jpg"), "note should have title");
        assert!(
            note.contains("attachment:"),
            "note should declare attachment"
        );
        assert!(
            note.contains("attachment_of:"),
            "note should declare attachment_of"
        );

        test_helpers::clear_test_written_files();
    }

    #[test]
    fn import_writer_treats_blank_parent_path_as_workspace_root_for_grafting() {
        let root_index = "---\ntitle: Home\nlink_format: markdown_root\ncontents:\n  - '[Existing](/existing.md)'\n---\n";

        test_helpers::clear_test_written_files();
        test_helpers::write_file("index.md", root_index).expect("seed root index");

        let entry: ImportedEntry = serde_json::from_value(serde_json::json!({
            "title": "Root Import",
            "date": "2020-09-24T01:36:35Z",
            "body": "Imported into root.",
            "metadata": {},
            "attachments": []
        }))
        .expect("valid imported entry fixture");

        let result = orchestrate::write_entries("journal", &[entry], Some("   "));
        assert_eq!(result.imported, 1);

        let updated_root =
            test_helpers::get_test_written_text_file("index.md").expect("root index updated");
        assert!(updated_root.contains("[Existing](/existing.md)"));
        assert!(updated_root.contains("[Journal](/journal/index.md)"));

        let import_root = test_helpers::get_test_written_text_file("journal/index.md")
            .expect("import root index written");
        assert!(import_root.contains("part_of: '[Home](/index.md)'"));

        test_helpers::clear_test_written_files();
    }

    #[test]
    fn import_writer_respects_workspace_markdown_relative_link_format() {
        let root_index = "---\ntitle: Home\nlink_format: markdown_relative\ncontents: []\n---\n";

        test_helpers::clear_test_written_files();
        test_helpers::write_file("index.md", root_index).expect("seed root index");

        let entry: ImportedEntry = serde_json::from_value(serde_json::json!({
            "title": "Gallery Entry",
            "date": "2020-09-24T01:36:35Z",
            "body": "![](_attachments/pic 1.jpg)",
            "metadata": {},
            "attachments": [{
                "filename": "pic 1.jpg",
                "content_type": "image/jpeg",
                "data": [106, 112, 101, 103, 45, 98, 121, 116, 101, 115]
            }]
        }))
        .expect("valid imported entry fixture");

        let result = orchestrate::write_entries("imports", &[entry], None);
        assert_eq!(result.imported, 1);

        let entry_path = "imports/2020/09/2020-09-24-gallery-entry.md";
        let content =
            test_helpers::get_test_written_text_file(entry_path).expect("entry file written");
        assert!(content.contains("link:"), "entry should have a self-link");
        assert!(content.contains("part_of: '[2020-09](2020_09.md)'"));
        // Attachment frontmatter links point to note files, not binaries.
        assert!(
            content.contains("[pic 1.jpg](<2020-09-24-gallery-entry/_attachments/pic 1.jpg.md>)")
        );
        // Body image references still point to binary.
        assert!(content.contains("![](<2020-09-24-gallery-entry/_attachments/pic 1.jpg>)"));

        test_helpers::clear_test_written_files();
    }

    #[test]
    fn execute_typed_command_wraps_successful_parse_dayone_response() {
        let dayone_json = r##"{
            "metadata": { "version": "1.0" },
            "entries": [{
                "uuid": "ABC123",
                "text": "# Hello World\nThis is a test.",
                "creationDate": "2020-09-24T01:36:35Z",
                "starred": false,
                "tags": [],
                "isPinned": false
            }]
        }"##;

        let output = execute_typed_command_inner(
            serde_json::json!({
                "type": "ParseDayOne",
                "params": { "data": BASE64.encode(dayone_json.as_bytes()) },
            })
            .to_string()
            .as_str(),
        )
        .expect("typed command should succeed");

        let response: JsonValue = serde_json::from_str(&output).expect("response should be JSON");
        assert_eq!(
            response.get("type").and_then(|v| v.as_str()),
            Some("PluginResult")
        );

        let data = response
            .get("data")
            .expect("PluginResult should include data");
        let entries = data
            .get("entries")
            .and_then(|v| v.as_array())
            .expect("parse result should include entries array");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].get("title").and_then(|v| v.as_str()),
            Some("Hello World")
        );
    }

    #[test]
    fn execute_typed_command_returns_empty_for_unknown_commands() {
        let input = serde_json::json!({
            "type": "NotARealImportCommand",
            "params": {},
        })
        .to_string();

        let output = execute_typed_command_inner(input.as_str())
            .expect("unknown typed commands should return empty string");

        assert!(output.is_empty());
    }

    #[test]
    fn execute_typed_command_surfaces_command_errors() {
        let input = serde_json::json!({
            "type": "ParseDayOne",
            "params": {},
        })
        .to_string();

        let error = execute_typed_command_inner(input.as_str())
            .expect_err("invalid ParseDayOne params should error");

        assert!(error.to_string().contains("Missing 'data' parameter"));
    }
}

fn handle_import_entries(params: JsonValue) -> CommandResponse {
    let entries_json = match params.get("entries_json").and_then(|v| v.as_str()) {
        Some(j) => j,
        None => {
            return CommandResponse::err("Missing 'entries_json' parameter");
        }
    };

    let folder = match params.get("folder").and_then(|v| v.as_str()) {
        Some(f) => f,
        None => {
            return CommandResponse::err("Missing 'folder' parameter");
        }
    };

    let parent_path = params.get("parent_path").and_then(|v| v.as_str());

    let entries: Vec<ImportedEntry> = match serde_json::from_str(entries_json) {
        Ok(e) => e,
        Err(e) => {
            return CommandResponse::err(format!("Invalid entries JSON: {e}"));
        }
    };

    let result = orchestrate::write_entries(folder, &entries, parent_path);

    match serde_json::to_value(&result) {
        Ok(data) => CommandResponse::ok(data),
        Err(e) => CommandResponse::err(format!("Failed to serialize result: {e}")),
    }
}

fn handle_import_directory_in_place(params: JsonValue) -> CommandResponse {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");

    match directory::import_directory_in_place(path) {
        Ok(result) => match serde_json::to_value(&result) {
            Ok(data) => CommandResponse::ok(data),
            Err(e) => CommandResponse::err(format!("Failed to serialize result: {e}")),
        },
        Err(e) => CommandResponse::err(e),
    }
}
