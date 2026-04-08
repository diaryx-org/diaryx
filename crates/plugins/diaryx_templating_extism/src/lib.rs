//! Extism guest plugin for Diaryx templating functionality.
//!
//! Provides creation-time template CRUD and render-time body templating
//! via Handlebars as an Extism WASM guest plugin.

mod creation;
mod render;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::creation::{Template, TemplateContext, TemplateInfo};
use chrono::{DateTime, FixedOffset};
use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

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
    match host::storage::get(&key) {
        Ok(Some(data)) => serde_json::from_slice(&data).unwrap_or_default(),
        _ => TemplatingConfig::default(),
    }
}

fn save_workspace_config(state: &TemplatingState) -> Result<(), String> {
    let key = storage_key_for_workspace(state.workspace_root.as_deref());
    let data = serde_json::to_vec(&state.config).map_err(|e| format!("serialize config: {e}"))?;
    host::storage::set(&key, &data)
}

fn update_workspace_root(workspace_root: Option<String>) -> Result<(), String> {
    let mut guard = state()
        .lock()
        .map_err(|_| "templating plugin state lock poisoned".to_string())?;
    guard.workspace_root = workspace_root.clone();
    guard.config = load_workspace_config(workspace_root.as_deref());
    Ok(())
}

fn current_local_datetime() -> Result<DateTime<FixedOffset>, String> {
    let raw = host::time::now_rfc3339()?;
    DateTime::parse_from_rfc3339(&raw)
        .map_err(|e| format!("failed to parse host_get_now response: {e}"))
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
    if let Ok(files) = host::fs::list_files(&dir) {
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

    if let Ok(true) = host::fs::file_exists(&template_path) {
        return host::fs::read_file(&template_path);
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
    host::fs::write_file(&template_path, content)
}

fn delete_template_impl(state: &TemplatingState, name: &str) -> Result<(), String> {
    let dir = templates_dir(state);
    let template_path = format!("{}/{}.md", dir, name);
    host::fs::delete_file(&template_path)
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
        "GetTemplatePath" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or("GetTemplatePath requires 'name' param")?;
            let dir = templates_dir(&state);
            let template_path = format!("{}/{}.md", dir, name);
            if host::fs::file_exists(&template_path).unwrap_or(false) {
                Ok(serde_json::json!({ "path": template_path, "source": "workspace" }))
            } else if name == "note" {
                Ok(serde_json::json!({ "source": "builtin" }))
            } else {
                Err(format!("Template not found: {name}"))
            }
        }
        "GetTemplateVariables" => Ok(serde_json::json!(creation::TEMPLATE_VARIABLES)),
        "GetTemplatePaths" => {
            let dir = templates_dir(&state);
            Ok(serde_json::json!({ "workspace_templates_dir": dir }))
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

            let rendered = if let Some(aud) = audience {
                render::render_for_audience(
                    body,
                    &frontmatter,
                    Path::new(file_path),
                    workspace_root.map(Path::new),
                    aud,
                )?
            } else {
                render::render(
                    body,
                    &frontmatter,
                    Path::new(file_path),
                    workspace_root.map(Path::new),
                )?
            };
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
            let part_of = params.get("part_of").and_then(|v| v.as_str());

            let content = get_template_impl(&state, template_name)?;
            let template = Template::new(template_name, content);

            let mut ctx = TemplateContext::new();
            if let Some(t) = title {
                ctx = ctx.with_title(t);
            }
            if let Some(f) = filename {
                ctx = ctx.with_filename(f);
            }
            if let Some(p) = part_of {
                ctx = ctx.with_part_of(p);
            }

            // Pass through any custom variables
            if let Some(custom) = params.get("custom").and_then(|v| v.as_object()) {
                for (k, v) in custom {
                    if let Some(val) = v.as_str() {
                        ctx = ctx.with_custom(k.as_str(), val);
                    }
                }
            }

            let now = current_local_datetime()?;
            let rendered = template.render(&ctx, &now);
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
        "GetTemplatePath".into(),
        "GetTemplateVariables".into(),
        "GetTemplatePaths".into(),
        "get_component_html".into(),
    ]
}

// ============================================================================
// Plugin exports
// ============================================================================

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.templating",
        "Templating",
        env!("CARGO_PKG_VERSION"),
        "Creation-time templates and render-time body templating with Handlebars",
        vec!["workspace_events".into(), "custom_commands".into()],
    )
    .min_app_version("1.4.1")
    .ui(vec![
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
    ])
    .commands(all_commands())
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] },
            "delete_files": { "include": ["all"], "exclude": [] },
            "plugin_storage": { "include": ["all"], "exclude": [] }
        }),
        reasons: HashMap::from([
            ("read_files".into(), "Read workspace templates from the _templates directory.".into()),
            ("edit_files".into(), "Update existing workspace templates when saving changes.".into()),
            ("create_files".into(), "Create new workspace templates in the _templates directory.".into()),
            ("delete_files".into(), "Remove workspace templates that are no longer needed.".into()),
            ("plugin_storage".into(), "Persist templating plugin configuration for the current workspace.".into()),
        ]),
    });

    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(input: String) -> FnResult<String> {
    let params: InitParams = serde_json::from_str(&input).unwrap_or_default();
    update_workspace_root(params.workspace_root).map_err(extism_pdk::Error::msg)?;
    host::log::log("info", "Templating plugin initialized");
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    host::log::log("info", "Templating plugin shutdown");
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = match dispatch_command(&req.command, req.params) {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
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
