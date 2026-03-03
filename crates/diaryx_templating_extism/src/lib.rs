//! Extism guest plugin for Diaryx templating functionality.
//!
//! Provides creation-time template CRUD and render-time body templating
//! via Handlebars as an Extism WASM guest plugin.

pub mod host_bridge;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use diaryx_templating::creation::{self, Template, TemplateContext, TemplateInfo};
use diaryx_templating::render::{self, BodyTemplateRenderer};
use extism_pdk::*;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

// ============================================================================
// Guest manifest / protocol types
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

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct InitParams {
    #[serde(default)]
    workspace_root: Option<String>,
}

// ============================================================================
// Plugin state
// ============================================================================

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct TemplatingConfig {
    /// Default template name for new entries.
    #[serde(default)]
    pub default_template: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TemplatingState {
    workspace_root: Option<String>,
    config: TemplatingConfig,
}

static STATE: OnceLock<Mutex<TemplatingState>> = OnceLock::new();

fn state() -> &'static Mutex<TemplatingState> {
    STATE.get_or_init(|| Mutex::new(TemplatingState::default()))
}

fn current_state() -> Result<TemplatingState, String> {
    let guard = state()
        .lock()
        .map_err(|_| "templating plugin state lock poisoned".to_string())?;
    Ok(guard.clone())
}

fn storage_key_for_workspace(workspace_root: Option<&str>) -> String {
    let token = workspace_root.unwrap_or("__default__");
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    format!("templating.config.{:x}", hasher.finish())
}

fn load_workspace_config(workspace_root: Option<&str>) -> TemplatingConfig {
    let key = storage_key_for_workspace(workspace_root);
    match host_bridge::storage_get(&key) {
        Ok(Some(data)) => serde_json::from_slice(&data).unwrap_or_default(),
        _ => TemplatingConfig::default(),
    }
}

fn save_workspace_config(state: &TemplatingState) -> Result<(), String> {
    let key = storage_key_for_workspace(state.workspace_root.as_deref());
    let data = serde_json::to_vec(&state.config).map_err(|e| format!("serialize config: {e}"))?;
    host_bridge::storage_set(&key, &data)
}

fn update_workspace_root(workspace_root: Option<String>) -> Result<(), String> {
    let mut guard = state()
        .lock()
        .map_err(|_| "templating plugin state lock poisoned".to_string())?;
    guard.workspace_root = workspace_root.clone();
    guard.config = load_workspace_config(workspace_root.as_deref());
    Ok(())
}

// ============================================================================
// Path helpers
// ============================================================================

fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn is_absolute_path(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return true;
    }
    path.len() > 1 && path.as_bytes()[1] == b':'
}

fn to_fs_path(rel_path: &str, workspace_root: Option<&str>) -> String {
    let rel = normalize_rel_path(rel_path);
    match workspace_root {
        Some(root) if !root.trim().is_empty() => {
            if root.ends_with(".md") {
                if let Some(parent) = Path::new(root).parent() {
                    return parent.join(&rel).to_string_lossy().to_string();
                }
            }
            if is_absolute_path(root) {
                return Path::new(root).join(&rel).to_string_lossy().to_string();
            }
            if root == "." {
                rel
            } else {
                Path::new(root).join(&rel).to_string_lossy().to_string()
            }
        }
        _ => rel,
    }
}

// ============================================================================
// Template CRUD helpers
// ============================================================================

fn templates_dir(state: &TemplatingState) -> String {
    to_fs_path("_templates", state.workspace_root.as_deref())
}

fn list_templates_impl(state: &TemplatingState) -> Result<Vec<TemplateInfo>, String> {
    let mut templates = Vec::new();

    // Built-in templates
    templates.push(TemplateInfo {
        name: "note".to_string(),
        source: "builtin".to_string(),
    });

    // Workspace templates from _templates/ directory
    let dir = templates_dir(state);
    if let Ok(files) = host_bridge::list_files(&dir) {
        for file_path in files {
            if file_path.ends_with(".md") {
                if let Some(name) = Path::new(&file_path).file_stem().and_then(|s| s.to_str()) {
                    templates.push(TemplateInfo {
                        name: name.to_string(),
                        source: "workspace".to_string(),
                    });
                }
            }
        }
    }

    Ok(templates)
}

fn get_template_impl(state: &TemplatingState, name: &str) -> Result<String, String> {
    // Check workspace templates first
    let dir = templates_dir(state);
    let template_path = format!("{}/{}.md", dir, name);

    if let Ok(true) = host_bridge::file_exists(&template_path) {
        return host_bridge::read_file(&template_path);
    }

    // Return built-in template
    match name {
        "note" => Ok(creation::DEFAULT_NOTE_TEMPLATE.to_string()),
        _ => Err(format!("Template not found: {name}")),
    }
}

fn save_template_impl(state: &TemplatingState, name: &str, content: &str) -> Result<(), String> {
    let dir = templates_dir(state);
    let template_path = format!("{}/{}.md", dir, name);
    host_bridge::write_file(&template_path, content)
}

fn delete_template_impl(state: &TemplatingState, name: &str) -> Result<(), String> {
    let dir = templates_dir(state);
    let template_path = format!("{}/{}.md", dir, name);
    host_bridge::delete_file(&template_path)
}

// ============================================================================
// Command dispatch
// ============================================================================

fn dispatch_command(command: &str, params: JsonValue) -> Result<JsonValue, String> {
    let state = current_state()?;

    match command {
        "ListTemplates" => {
            let templates = list_templates_impl(&state)?;
            serde_json::to_value(&templates).map_err(|e| format!("serialize templates: {e}"))
        }
        "GetTemplate" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("GetTemplate requires 'name' param")?;
            let content = get_template_impl(&state, name)?;
            Ok(JsonValue::String(content))
        }
        "SaveTemplate" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("SaveTemplate requires 'name' param")?;
            let content = params
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or("SaveTemplate requires 'content' param")?;
            save_template_impl(&state, name, content)?;
            Ok(JsonValue::Null)
        }
        "DeleteTemplate" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("DeleteTemplate requires 'name' param")?;
            delete_template_impl(&state, name)?;
            Ok(JsonValue::Null)
        }
        "RenderBody" => {
            let body = params
                .get("body")
                .and_then(|v| v.as_str())
                .ok_or("RenderBody requires 'body' param")?;
            let frontmatter_json = params
                .get("frontmatter")
                .cloned()
                .unwrap_or(JsonValue::Object(serde_json::Map::new()));
            let file_path = params
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("entry.md");
            let workspace_root = params.get("workspace_root").and_then(|v| v.as_str());
            let audience = params.get("audience").and_then(|v| v.as_str());

            // Convert JSON frontmatter to YAML IndexMap for the render API
            let frontmatter: IndexMap<String, YamlValue> =
                if let JsonValue::Object(map) = &frontmatter_json {
                    map.iter()
                        .map(|(k, v)| (k.clone(), json_to_yaml(v)))
                        .collect()
                } else {
                    IndexMap::new()
                };

            let context = if let Some(aud) = audience {
                render::build_publish_context(
                    &frontmatter,
                    Path::new(file_path),
                    workspace_root.map(Path::new),
                    aud,
                )
            } else {
                render::build_context(
                    &frontmatter,
                    Path::new(file_path),
                    workspace_root.map(Path::new),
                )
            };

            let renderer = BodyTemplateRenderer::new();
            let rendered = renderer.render(body, &context)?;
            Ok(JsonValue::String(rendered))
        }
        "HasTemplates" => {
            let body = params
                .get("body")
                .and_then(|v| v.as_str())
                .ok_or("HasTemplates requires 'body' param")?;
            Ok(JsonValue::Bool(render::has_templates(body)))
        }
        "RenderCreationTemplate" => {
            let template_name = params
                .get("template")
                .and_then(|v| v.as_str())
                .unwrap_or("note");
            let title = params.get("title").and_then(|v| v.as_str());
            let filename = params.get("filename").and_then(|v| v.as_str());

            let content = get_template_impl(&state, template_name)?;
            let template = Template::new(template_name, content);

            let mut ctx = TemplateContext::new();
            if let Some(t) = title {
                ctx = ctx.with_title(t);
            }
            if let Some(f) = filename {
                ctx = ctx.with_filename(f);
            }

            let rendered = template.render(&ctx);
            Ok(JsonValue::String(rendered))
        }
        "get_component_html" => {
            let component_id = params
                .get("component_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match component_id {
                "templating.settings" => Ok(serde_json::json!({
                    "html": include_str!("ui/settings.html"),
                })),
                _ => Err(format!("Unknown component: {component_id}")),
            }
        }
        _ => Err(format!("Unknown command: {command}")),
    }
}

/// Convert `serde_json::Value` to `serde_yaml::Value`.
fn json_to_yaml(value: &JsonValue) -> YamlValue {
    match value {
        JsonValue::Null => YamlValue::Null,
        JsonValue::Bool(b) => YamlValue::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                YamlValue::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                YamlValue::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                YamlValue::Number(serde_yaml::Number::from(f))
            } else {
                YamlValue::Null
            }
        }
        JsonValue::String(s) => YamlValue::String(s.clone()),
        JsonValue::Array(arr) => YamlValue::Sequence(arr.iter().map(json_to_yaml).collect()),
        JsonValue::Object(map) => {
            let mapping: serde_yaml::Mapping = map
                .iter()
                .map(|(k, v)| (YamlValue::String(k.clone()), json_to_yaml(v)))
                .collect();
            YamlValue::Mapping(mapping)
        }
    }
}

fn all_commands() -> Vec<String> {
    vec![
        "ListTemplates".into(),
        "GetTemplate".into(),
        "SaveTemplate".into(),
        "DeleteTemplate".into(),
        "RenderBody".into(),
        "HasTemplates".into(),
        "RenderCreationTemplate".into(),
        "get_component_html".into(),
    ]
}

// ============================================================================
// Plugin exports
// ============================================================================

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest {
        id: "diaryx.templating".into(),
        name: "Templating".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Creation-time templates and render-time body templating with Handlebars"
            .into(),
        capabilities: vec!["workspace_events".into(), "custom_commands".into()],
        ui: vec![
            serde_json::json!({
                "slot": "SettingsTab",
                "id": "templating-settings",
                "label": "Templates",
                "icon": "file-code",
                "fields": [],
                "component": {
                    "type": "Iframe",
                    "component_id": "templating.settings",
                },
            }),
            serde_json::json!({
                "slot": "EditorExtension",
                "extension_id": "templateVariable",
                "node_type": { "Builtin": { "host_extension_id": "templateVariable" } },
                "markdown": { "level": "Inline", "open": "{{", "close": "}}" },
            }),
            serde_json::json!({
                "slot": "EditorExtension",
                "extension_id": "conditionalBlock",
                "node_type": { "Builtin": { "host_extension_id": "conditionalBlock" } },
                "markdown": { "level": "Block", "open": "{{#", "close": "}}" },
            }),
            serde_json::json!({
                "slot": "BlockPickerItem",
                "id": "templating-if-else",
                "label": "If / Else",
                "icon": "git-branch",
                "editor_command": "insertConditionalBlock",
                "params": { "helperType": "if" },
                "prompt": { "message": "Variable name to check:", "default_value": "draft", "param_key": "condition" },
            }),
            serde_json::json!({
                "slot": "BlockPickerItem",
                "id": "templating-for-audience",
                "label": "For Audience",
                "icon": "users",
                "editor_command": "insertConditionalBlock",
                "params": { "helperType": "for-audience" },
                "prompt": { "message": "Audience name:", "default_value": "public", "param_key": "condition" },
            }),
        ],
        commands: all_commands(),
        cli: vec![],
    };

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    update_workspace_root(params.workspace_root).map_err(extism_pdk::Error::msg)?;
    host_bridge::log_message("info", "Templating plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host_bridge::log_message("info", "Templating plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = match dispatch_command(&req.command, req.params) {
        Ok(data) => CommandResponse {
            success: true,
            data: Some(data),
            error: None,
        },
        Err(error) => CommandResponse {
            success: false,
            data: None,
            error: Some(error),
        },
    };
    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let state = current_state().map_err(extism_pdk::Error::msg)?;
    Ok(serde_json::to_string(&state.config)?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let mut guard = state()
        .lock()
        .map_err(|_| extism_pdk::Error::msg("templating plugin state lock poisoned"))?;
    let config: TemplatingConfig = serde_json::from_str(&input).unwrap_or_default();
    guard.config = config;
    save_workspace_config(&guard).map_err(extism_pdk::Error::msg)?;
    Ok(String::new())
}

#[plugin_fn]
pub fn on_event(input: String) -> FnResult<String> {
    let event: JsonValue = serde_json::from_str(&input).unwrap_or(JsonValue::Null);
    let event_type = event
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if matches!(event_type, "workspace_opened" | "workspace_changed") {
        let workspace_root = event
            .get("payload")
            .and_then(|v| v.get("workspace_root"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let _ = update_workspace_root(workspace_root);
    }

    Ok(String::new())
}
