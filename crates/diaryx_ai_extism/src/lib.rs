//! AI chat assistant Extism guest plugin for Diaryx.
//!
//! Provides an AI chat sidebar powered by OpenAI-compatible APIs.
//! The plugin renders its own HTML UI via the `Iframe` ComponentRef
//! and communicates with the API through the `host_http_request` host function.

pub mod chat;

use extism_pdk::*;
use serde_json::Value as JsonValue;

// ============================================================================
// Protocol types (mirrors diaryx_extism::protocol)
// ============================================================================

#[derive(serde::Serialize, serde::Deserialize)]
struct GuestManifest {
    id: String,
    name: String,
    version: String,
    description: String,
    capabilities: Vec<String>,
    #[serde(default)]
    requested_permissions: Option<JsonValue>,
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
pub struct CommandResponse {
    pub success: bool,
    #[serde(default)]
    pub data: Option<JsonValue>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Plugin configuration stored via host_storage_get/set.
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
// Host storage FFI + reusable helpers
// ============================================================================

#[link(wasm_import_module = "extism:host/user")]
unsafe extern "C" {
    fn host_storage_get(offset: u64) -> u64;
    fn host_storage_set(offset: u64) -> u64;
}

/// Read bytes from host storage by key (base64-decoded).
pub(crate) fn storage_get(key: &str) -> Option<Vec<u8>> {
    let input = serde_json::json!({ "key": key });
    let input_str = serde_json::to_string(&input).unwrap_or_default();

    let result = unsafe {
        let mem = Memory::from_bytes(input_str.as_bytes()).expect("failed to allocate memory");
        let result_offset = host_storage_get(mem.offset());
        Memory::find(result_offset)
            .map(|m| String::from_utf8(m.to_vec()).unwrap_or_default())
            .unwrap_or_default()
    };

    if result.is_empty() {
        return None;
    }

    #[derive(serde::Deserialize)]
    struct StorageResult {
        #[serde(default)]
        data: Option<String>,
    }

    if let Ok(storage) = serde_json::from_str::<StorageResult>(&result) {
        if let Some(b64) = storage.data {
            return base64_decode(&b64).ok();
        }
    }

    None
}

/// Write bytes to host storage by key (base64-encoded).
pub(crate) fn storage_set(key: &str, data: &[u8]) {
    let b64 = base64_encode(data);
    let input = serde_json::json!({ "key": key, "data": b64 });
    let input_str = serde_json::to_string(&input).unwrap_or_default();

    unsafe {
        let mem = Memory::from_bytes(input_str.as_bytes()).expect("failed to allocate memory");
        host_storage_set(mem.offset());
    }
}

fn load_config() -> PluginConfig {
    storage_get("diaryx.ai.config")
        .and_then(|bytes| serde_json::from_slice::<PluginConfig>(&bytes).ok())
        .unwrap_or_default()
}

fn save_config(config: &PluginConfig) {
    let json_bytes = serde_json::to_vec(config).unwrap_or_default();
    storage_set("diaryx.ai.config", &json_bytes);
}

// Minimal base64 encode/decode (no dependency needed)
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    fn decode_char(c: u8) -> Result<u8, &'static str> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err("invalid base64 char"),
        }
    }

    let input = input.trim_end_matches('=');
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = decode_char(bytes[i])? as u32;
        let b1 = if i + 1 < bytes.len() {
            decode_char(bytes[i + 1])? as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            decode_char(bytes[i + 2])? as u32
        } else {
            0
        };
        let b3 = if i + 3 < bytes.len() {
            decode_char(bytes[i + 3])? as u32
        } else {
            0
        };
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        result.push((triple >> 16) as u8);
        if i + 2 < bytes.len() {
            result.push((triple >> 8) as u8);
        }
        if i + 3 < bytes.len() {
            result.push(triple as u8);
        }
        i += 4;
    }
    Ok(result)
}

// ============================================================================
// Guest exports
// ============================================================================

/// Return the plugin manifest.
#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest {
        id: "diaryx.ai".into(),
        name: "AI Assistant".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "AI chat assistant powered by OpenAI-compatible APIs".into(),
        capabilities: vec!["custom_commands".into()],
        requested_permissions: Some(serde_json::json!({
            "defaults": {
                "http_requests": {
                    "include": ["openrouter.ai"],
                    "exclude": []
                },
                "plugin_storage": {
                    "include": ["all"],
                    "exclude": []
                }
            },
            "reasons": {
                "http_requests": "Send chat requests to the configured OpenAI-compatible API endpoint.",
                "plugin_storage": "Persist conversation history and plugin settings between sessions."
            }
        })),
        ui: vec![
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
        ],
        commands: vec![
            "chat".into(),
            "chat_continue".into(),
            "clear_conversation".into(),
            "get_history".into(),
            "get_component_html".into(),
            "list_conversations".into(),
            "switch_conversation".into(),
            "new_conversation".into(),
            "delete_conversation".into(),
        ],
        cli: vec![],
    };

    Ok(serde_json::to_string(&manifest)?)
}

/// Handle commands dispatched by the host.
#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let request: CommandRequest = serde_json::from_str(&input).map_err(extism_pdk::Error::msg)?;

    let response = match request.command.as_str() {
        "get_component_html" => {
            // Return the chat UI HTML
            CommandResponse {
                success: true,
                data: Some(JsonValue::String(include_str!("ui.html").to_string())),
                error: None,
            }
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
        "new_conversation" => chat::new_conversation(),
        "delete_conversation" => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            chat::delete_conversation(id)
        }
        _ => CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Unknown command: {}", request.command)),
        },
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
