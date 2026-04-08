//! AI chat assistant Extism guest plugin for Diaryx.
//!
//! Provides an AI chat sidebar powered by OpenAI-compatible APIs.
//! The plugin renders its own HTML UI via the `Iframe` ComponentRef
//! and communicates with the API through the `host_http_request` host function.

pub mod chat;

use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Plugin configuration stored via host storage.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct PluginConfig {
    pub provider_mode: Option<String>,
    pub managed_model: Option<String>,
    pub api_endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

// ============================================================================
// Config helpers
// ============================================================================

fn load_config() -> PluginConfig {
    host::storage::get_json::<PluginConfig>("diaryx.ai.config")
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn save_config(config: &PluginConfig) {
    let _ = host::storage::set_json("diaryx.ai.config", config);
}

fn is_managed_mode(config: &PluginConfig) -> bool {
    config
        .provider_mode
        .as_deref()
        .map(|mode| mode.eq_ignore_ascii_case("managed"))
        .unwrap_or(true)
}

fn config_from_update_params(params: &JsonValue) -> PluginConfig {
    params
        .get("config")
        .cloned()
        .and_then(|value| serde_json::from_value::<PluginConfig>(value).ok())
        .unwrap_or_else(load_config)
}

fn build_update_config_data(params: &JsonValue) -> Option<JsonValue> {
    let config = config_from_update_params(params);
    if !is_managed_mode(&config) {
        return None;
    }

    let hostname = params
        .get("server_hostname")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    Some(serde_json::json!({
        "plugin_permissions_patch": {
            "plugin_id": "diaryx.ai",
            "mode": "merge",
            "permissions": {
                "http_requests": {
                    "include": [hostname],
                    "exclude": []
                }
            }
        }
    }))
}

// ============================================================================
// Guest exports
// ============================================================================

/// Return the plugin manifest.
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.ai",
        "AI Assistant",
        env!("CARGO_PKG_VERSION"),
        "AI chat assistant powered by OpenAI-compatible APIs",
        vec!["custom_commands".into()],
    )
    .ui(vec![
        // Toolbar button to toggle the AI sidebar
        serde_json::json!({
            "slot": "ToolbarButton",
            "id": "ai-chat-toggle",
            "label": "AI Assistant",
            "icon": "sparkles",
            "plugin_command": "toggle_ai",
        }),
        // Right sidebar tab with iframe-based chat UI
        serde_json::json!({
            "slot": "SidebarTab",
            "id": "ai-chat",
            "label": "AI",
            "icon": "sparkles",
            "side": "Right",
            "component": {
                "type": "Iframe",
                "component_id": "ai-chat-ui",
            },
        }),
        // Settings tab for API configuration
        serde_json::json!({
            "slot": "SettingsTab",
            "id": "ai-settings",
            "label": "AI",
            "icon": "sparkles",
            "fields": [
                {
                    "type": "Section",
                    "label": "AI Configuration",
                    "description": "Configure your AI assistant API connection",
                },
                {
                    "type": "Select",
                    "key": "provider_mode",
                    "label": "Provider",
                    "description": "Choose Diaryx Plus managed AI or use your own API key",
                    "options": [
                        { "value": "managed", "label": "Diaryx Plus (managed)" },
                        { "value": "byo", "label": "Bring your own API key" }
                    ],
                },
                {
                    "type": "Select",
                    "key": "managed_model",
                    "label": "Diaryx Plus Model",
                    "description": "Model used for managed mode",
                    "options": [
                        { "value": "google/gemini-3-flash-preview", "label": "Gemini 3 Flash Preview" },
                        { "value": "anthropic/claude-haiku-4.5", "label": "Claude Haiku 4.5" },
                        { "value": "openai/gpt-5.2", "label": "OpenAI GPT-5.2" }
                    ],
                },
                {
                    "type": "Text",
                    "key": "api_endpoint",
                    "label": "API Endpoint",
                    "description": "OpenAI-compatible endpoint for BYO mode (default: OpenRouter)",
                },
                {
                    "type": "Text",
                    "key": "api_key",
                    "label": "API Key",
                    "description": "Your API key (BYO mode only)",
                },
                {
                    "type": "Text",
                    "key": "model",
                    "label": "Model",
                    "description": "BYO model, e.g. anthropic/claude-sonnet-4-6",
                },
                {
                    "type": "Text",
                    "key": "system_prompt",
                    "label": "System Prompt",
                    "description": "Custom instructions for the AI",
                },
            ],
        }),
    ])
    .commands(vec![
        "chat".into(),
        "chat_continue".into(),
        "clear_conversation".into(),
        "get_history".into(),
        "get_component_html".into(),
        "list_conversations".into(),
        "switch_conversation".into(),
        "new_conversation".into(),
        "delete_conversation".into(),
        "UpdateConfig".into(),
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: serde_json::json!({
            "http_requests": {
                "include": ["openrouter.ai"],
                "exclude": []
            },
            "read_files": {
                "include": ["all"],
                "exclude": []
            },
            "edit_files": {
                "include": ["all"],
                "exclude": []
            },
            "plugin_storage": {
                "include": ["all"],
                "exclude": []
            }
        }),
        reasons: HashMap::from([
            ("http_requests".into(), "Send chat requests to the configured OpenAI-compatible API endpoint.".into()),
            ("plugin_storage".into(), "Persist conversation history and plugin settings between sessions.".into()),
            ("read_files".into(), "Read existing conversation files so AI chat saves preserve Diaryx frontmatter and hierarchy metadata.".into()),
            ("edit_files".into(), "Update the selected workspace conversation file with the latest chat transcript.".into()),
        ]),
    });

    Ok(serde_json::to_string(&manifest)?)
}

/// Handle commands dispatched by the host.
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let request: CommandRequest = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let response = match request.command.as_str() {
        "get_component_html" => {
            // Return the chat UI HTML
            CommandResponse::ok(JsonValue::String(include_str!("ui.html").to_string()))
        }
        "chat" => {
            let chat_input: chat::ChatInput =
                serde_json::from_value(request.params).unwrap_or(chat::ChatInput {
                    message: String::new(),
                    entries: Vec::new(),
                    managed: None,
                });
            let config = load_config();
            chat::handle_chat(chat_input, &config)
        }
        "chat_continue" => {
            let managed = request
                .params
                .get("managed")
                .cloned()
                .and_then(|v| serde_json::from_value::<chat::ManagedContext>(v).ok());
            chat::chat_continue(managed)
        }
        "clear_conversation" => chat::clear_conversation(),
        "get_history" => chat::get_history(),
        "list_conversations" => chat::list_conversations(),
        "switch_conversation" => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            chat::switch_conversation(id)
        }
        "new_conversation" => {
            let title = request
                .params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let file_path = request
                .params
                .get("file_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            chat::new_conversation(title, file_path)
        }
        "delete_conversation" => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            chat::delete_conversation(id)
        }
        "UpdateConfig" => CommandResponse {
            success: true,
            data: build_update_config_data(&request.params),
            error: None,
            error_code: None,
        },
        _ => CommandResponse::err(format!("Unknown command: {}", request.command)),
    };

    Ok(serde_json::to_string(&response)?)
}

/// Handle lifecycle events (no-op for AI plugin).
#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

/// Get plugin configuration.
#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = load_config();
    Ok(serde_json::to_string(&config)?)
}

/// Set plugin configuration.
#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let config: PluginConfig = serde_json::from_str(&input).unwrap_or_default();
    save_config(&config);
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_config_generates_permission_patch_for_managed_mode() {
        let data = build_update_config_data(&serde_json::json!({
            "config": {
                "provider_mode": "managed"
            },
            "server_hostname": "sync.example"
        }))
        .expect("expected permission patch");

        assert_eq!(
            data["plugin_permissions_patch"]["mode"].as_str(),
            Some("merge")
        );
        assert_eq!(
            data["plugin_permissions_patch"]["permissions"]["http_requests"]["include"][0].as_str(),
            Some("sync.example")
        );
    }

    #[test]
    fn update_config_is_noop_for_non_managed_mode() {
        assert!(
            build_update_config_data(&serde_json::json!({
                "config": {
                    "provider_mode": "byo"
                },
                "server_hostname": "sync.example"
            }))
            .is_none()
        );
    }
}
