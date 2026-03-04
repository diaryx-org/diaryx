//! Chat logic: message history, context building, API calls, agent tool-use loop.
//!
//! The agent loop is **UI-driven**: after each batch of tool calls the plugin
//! returns to the iframe with `status: "tool_calls"` and the accumulated steps.
//! The iframe renders those steps then calls `chat_continue` to resume.  This
//! gives the user real-time feedback instead of waiting for the full loop.

use crate::{CommandResponse, PluginConfig, storage_get, storage_set};
use serde_json::Value as JsonValue;

const LEGACY_HISTORY_KEY: &str = "diaryx.ai.history";
const INDEX_KEY: &str = "diaryx.ai.conversations.index";
const CONV_PREFIX: &str = "diaryx.ai.conversations.";
const MAX_HISTORY_MESSAGES: usize = 50;
const MAX_CONVERSATIONS: usize = 50;
const MAX_AGENT_ITERATIONS: usize = 10;
const MAX_TOOL_RESULT_BYTES: usize = 8192;
const DEFAULT_BYO_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const DEFAULT_BYO_MODEL: &str = "anthropic/claude-sonnet-4-6";
const DEFAULT_MANAGED_MODEL: &str = "google/gemini-3-flash-preview";
const CHAT_HTTP_TIMEOUT_MS: u64 = 90_000;

// ============================================================================
// Types
// ============================================================================

/// A single message in the conversation (user/assistant text only).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Chat command input from the iframe.
#[derive(serde::Deserialize)]
pub struct ChatInput {
    pub message: String,
    #[serde(default)]
    pub entries: Vec<EntryContext>,
    #[serde(default)]
    pub managed: Option<ManagedContext>,
}

/// Managed-mode context injected by the host.
#[derive(serde::Deserialize, Clone, Default)]
pub struct ManagedContext {
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
}

/// Entry context attached to a chat request.
#[derive(serde::Deserialize)]
pub struct EntryContext {
    pub path: String,
    pub content: String,
}

/// Metadata for a single conversation in the index.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub message_count: usize,
}

/// Top-level index of all conversations.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct ConversationIndex {
    pub conversations: Vec<ConversationMeta>,
    pub active_id: Option<String>,
}

/// Tracks an agent tool-use step for the UI.
#[derive(serde::Serialize, Clone)]
pub struct AgentStep {
    #[serde(rename = "type")]
    pub step_type: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ============================================================================
// Conversation + agent state
// ============================================================================

/// Conversation state held in plugin memory.
pub struct Conversation {
    pub id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub loaded: bool,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            id: None,
            messages: Vec::new(),
            loaded: false,
        }
    }
}

/// In-flight agent state saved between `chat` → `chat_continue` roundtrips.
struct AgentState {
    /// The full API messages array (system + history + user + tool msgs).
    api_messages: Vec<JsonValue>,
    /// The original user message (for persisting to history at the end).
    user_message: String,
    /// Steps accumulated so far across all iterations.
    steps: Vec<AgentStep>,
    /// How many API calls we've made so far.
    iterations: usize,
    /// API config snapshot (so chat_continue doesn't need the config again).
    url: String,
    model: String,
    api_key: String,
}

std::thread_local! {
    static CONVERSATION: std::cell::RefCell<Conversation> = std::cell::RefCell::new(Conversation::new());
    static AGENT_STATE: std::cell::RefCell<Option<AgentState>> = const { std::cell::RefCell::new(None) };
}

// ============================================================================
// Conversation index & message persistence
// ============================================================================

fn load_index() -> ConversationIndex {
    storage_get(INDEX_KEY)
        .and_then(|bytes| serde_json::from_slice::<ConversationIndex>(&bytes).ok())
        .unwrap_or_default()
}

fn save_index(index: &ConversationIndex) {
    let bytes = serde_json::to_vec(index).unwrap_or_default();
    storage_set(INDEX_KEY, &bytes);
}

fn conv_storage_key(id: &str) -> String {
    format!("{}{}", CONV_PREFIX, id)
}

fn load_conversation_messages(id: &str) -> Vec<ChatMessage> {
    storage_get(&conv_storage_key(id))
        .and_then(|bytes| serde_json::from_slice::<Vec<ChatMessage>>(&bytes).ok())
        .unwrap_or_default()
}

fn save_conversation_messages(id: &str, messages: &[ChatMessage]) {
    let trimmed: &[ChatMessage] = if messages.len() > MAX_HISTORY_MESSAGES {
        let excess = messages.len() - MAX_HISTORY_MESSAGES;
        let start = if excess % 2 == 0 { excess } else { excess + 1 };
        &messages[start..]
    } else {
        messages
    };
    let bytes = serde_json::to_vec(trimmed).unwrap_or_default();
    storage_set(&conv_storage_key(id), &bytes);
}

fn delete_conversation_messages(id: &str) {
    storage_set(&conv_storage_key(id), &[]);
}

fn generate_conversation_id() -> String {
    let index = load_index();
    // Find the highest existing numeric suffix to avoid collisions after deletions
    let max_n = index
        .conversations
        .iter()
        .filter_map(|c| c.id.strip_prefix("conv_")?.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("conv_{}", max_n + 1)
}

fn generate_title(first_user_message: &str) -> String {
    let trimmed = first_user_message.trim();
    if trimmed.len() <= 50 {
        trimmed.to_string()
    } else {
        let mut end = 50;
        // Try to break at a word boundary
        if let Some(pos) = trimmed[..50].rfind(' ') {
            end = pos;
        }
        format!("{}...", &trimmed[..end])
    }
}

// ============================================================================
// Migration from flat history
// ============================================================================

fn migrate_if_needed() {
    // Already migrated?
    if storage_get(INDEX_KEY).is_some() {
        return;
    }

    // Check for legacy flat history
    let legacy = storage_get(LEGACY_HISTORY_KEY)
        .and_then(|bytes| serde_json::from_slice::<Vec<ChatMessage>>(&bytes).ok())
        .unwrap_or_default();

    if legacy.is_empty() {
        // Nothing to migrate — save an empty index so we don't check again
        save_index(&ConversationIndex::default());
        return;
    }

    let id = "conv_1".to_string();
    let title = legacy
        .iter()
        .find(|m| m.role == "user")
        .map(|m| generate_title(&m.content))
        .unwrap_or_else(|| "Imported chat".into());

    let meta = ConversationMeta {
        id: id.clone(),
        title,
        created_at: 0,
        message_count: legacy.len(),
    };

    save_conversation_messages(&id, &legacy);

    let index = ConversationIndex {
        conversations: vec![meta],
        active_id: Some(id),
    };
    save_index(&index);
}

// ============================================================================
// ensure_loaded (multi-conversation aware)
// ============================================================================

fn ensure_loaded() {
    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        if !conv.loaded {
            migrate_if_needed();
            let index = load_index();
            if let Some(active_id) = &index.active_id {
                conv.id = Some(active_id.clone());
                conv.messages = load_conversation_messages(active_id);
            }
            conv.loaded = true;
        }
    });
}

// ============================================================================
// Host function imports
// ============================================================================

#[link(wasm_import_module = "extism:host/user")]
unsafe extern "C" {
    fn host_http_request(offset: u64) -> u64;
    fn host_read_file(offset: u64) -> u64;
    fn host_list_files(offset: u64) -> u64;
}

fn call_host_http_request(input: &str) -> String {
    unsafe {
        let mem = extism_pdk::Memory::from_bytes(input.as_bytes())
            .expect("failed to allocate memory for http request");
        let result_offset = host_http_request(mem.offset());
        extism_pdk::Memory::find(result_offset)
            .map(|m| String::from_utf8(m.to_vec()).unwrap_or_default())
            .unwrap_or_default()
    }
}

fn call_host_read_file(path: &str) -> String {
    let input = serde_json::json!({ "path": path }).to_string();
    unsafe {
        let mem = extism_pdk::Memory::from_bytes(input.as_bytes())
            .expect("failed to allocate memory for read_file");
        let result_offset = host_read_file(mem.offset());
        extism_pdk::Memory::find(result_offset)
            .map(|m| String::from_utf8(m.to_vec()).unwrap_or_default())
            .unwrap_or_default()
    }
}

fn call_host_list_files(prefix: &str) -> String {
    let input = serde_json::json!({ "prefix": prefix }).to_string();
    unsafe {
        let mem = extism_pdk::Memory::from_bytes(input.as_bytes())
            .expect("failed to allocate memory for list_files");
        let result_offset = host_list_files(mem.offset());
        extism_pdk::Memory::find(result_offset)
            .map(|m| String::from_utf8(m.to_vec()).unwrap_or_default())
            .unwrap_or_default()
    }
}

// ============================================================================
// Tool definitions (OpenAI format)
// ============================================================================

fn build_tool_definitions() -> Vec<JsonValue> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the user's workspace. Returns the file content as text.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read, relative to the workspace root"
                        }
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List files in the user's workspace. Returns an array of file paths.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prefix": {
                            "type": "string",
                            "description": "Optional path prefix to filter files. Use empty string or omit to list all files."
                        }
                    }
                }
            }
        }),
    ]
}

// ============================================================================
// Tool execution
// ============================================================================

/// Execute a tool call, returning (result_content, summary).
fn execute_tool_call(name: &str, arguments_json: &str) -> (String, String) {
    let args: JsonValue = serde_json::from_str(arguments_json).unwrap_or_default();

    match name {
        "read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                return ("Error: missing 'path' argument".into(), "error".into());
            }
            let content = call_host_read_file(path);
            if content.is_empty() {
                return (
                    format!("File not found or empty: {}", path),
                    "not found".into(),
                );
            }
            let summary = format!("{} chars", content.len());
            let truncated = truncate_result(&content);
            (truncated, summary)
        }
        "list_files" => {
            let prefix = args.get("prefix").and_then(|v| v.as_str()).unwrap_or("");
            let result = call_host_list_files(prefix);
            let summary = if let Ok(arr) = serde_json::from_str::<Vec<String>>(&result) {
                format!("{} files", arr.len())
            } else {
                "error".into()
            };
            let truncated = truncate_result(&result);
            (truncated, summary)
        }
        _ => (format!("Unknown tool: {}", name), "error".into()),
    }
}

fn truncate_result(s: &str) -> String {
    if s.len() <= MAX_TOOL_RESULT_BYTES {
        s.to_string()
    } else {
        let mut truncated = s[..MAX_TOOL_RESULT_BYTES].to_string();
        truncated.push_str("\n... [truncated]");
        truncated
    }
}

fn plugin_error(code: &str, message: impl Into<String>) -> CommandResponse {
    CommandResponse {
        success: false,
        data: Some(serde_json::json!({ "code": code })),
        error: Some(message.into()),
    }
}

fn is_managed_mode(config: &PluginConfig) -> bool {
    config
        .provider_mode
        .as_deref()
        .is_some_and(|mode| mode.eq_ignore_ascii_case("managed"))
}

fn build_byo_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{}/chat/completions", trimmed)
    }
}

fn build_managed_url(server_url: &str) -> String {
    format!(
        "{}/api/ai/chat/completions",
        server_url.trim_end_matches('/')
    )
}

// ============================================================================
// Single API call + tool dispatch (shared by handle_chat & chat_continue)
// ============================================================================

/// Result of one agent iteration.
enum IterationResult {
    /// The model returned tool calls — execute them and yield to the UI.
    ToolCalls,
    /// The model returned a final text response.
    Done(String),
    /// An error occurred.
    Error(String),
}

/// Make one API call, process tool calls if any, update the agent state.
/// Returns the iteration result.
fn run_one_iteration(state: &mut AgentState) -> IterationResult {
    let tools = build_tool_definitions();

    let request_body = serde_json::json!({
        "model": state.model,
        "messages": state.api_messages,
        "tools": tools,
        "stream": false,
    });

    let http_input = serde_json::json!({
        "url": &state.url,
        "method": "POST",
        "headers": {
            "Content-Type": "application/json",
            "Authorization": format!("Bearer {}", state.api_key),
        },
        "body": serde_json::to_string(&request_body).unwrap_or_default(),
        "timeout_ms": CHAT_HTTP_TIMEOUT_MS,
    });

    let http_result =
        call_host_http_request(&serde_json::to_string(&http_input).unwrap_or_default());

    let http_response: JsonValue = serde_json::from_str(&http_result).unwrap_or_default();

    let status = http_response
        .get("status")
        .and_then(|s| s.as_u64())
        .unwrap_or(0);

    if status < 200 || status >= 300 {
        let body = http_response
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or("Unknown error");
        return IterationResult::Error(format!("API error ({}): {}", status, body));
    }

    let body_str = http_response
        .get("body")
        .and_then(|b| b.as_str())
        .unwrap_or("");

    let api_response: JsonValue = serde_json::from_str(body_str).unwrap_or_default();

    let choice = match api_response.get("choices").and_then(|c| c.get(0)) {
        Some(c) => c,
        None => return IterationResult::Error("No choices in API response".into()),
    };

    let message = match choice.get("message") {
        Some(m) => m,
        None => return IterationResult::Error("No message in API choice".into()),
    };

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .unwrap_or("");

    let tool_calls = message.get("tool_calls").and_then(|tc| tc.as_array());

    if finish_reason == "tool_calls" || tool_calls.is_some_and(|tc| !tc.is_empty()) {
        // Append the assistant message (with tool_calls) to the conversation
        state.api_messages.push(message.clone());

        let tool_calls = tool_calls.unwrap_or(&Vec::new()).clone();

        for tc in &tool_calls {
            let tc_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let func = tc.get("function").unwrap_or(&JsonValue::Null);
            let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = func
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");

            let args_val: JsonValue = serde_json::from_str(arguments).unwrap_or(JsonValue::Null);

            state.steps.push(AgentStep {
                step_type: "tool_call".into(),
                tool: name.into(),
                args: Some(args_val),
                summary: None,
            });

            let (result_content, summary) = execute_tool_call(name, arguments);

            state.steps.push(AgentStep {
                step_type: "tool_result".into(),
                tool: name.into(),
                args: None,
                summary: Some(summary),
            });

            state.api_messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tc_id,
                "content": result_content,
            }));
        }

        state.iterations += 1;
        return IterationResult::ToolCalls;
    }

    // Text response — done
    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("No response from AI")
        .to_string();

    IterationResult::Done(content)
}

/// Build the response JSON for a tool-calls yield or a final response.
fn build_response(state: &AgentState, final_text: Option<&str>) -> CommandResponse {
    let status = if final_text.is_some() {
        "done"
    } else {
        "tool_calls"
    };

    let mut data = serde_json::json!({ "status": status });

    if let Some(text) = final_text {
        data["response"] = JsonValue::String(text.to_string());
    }

    if !state.steps.is_empty() {
        data["steps"] = serde_json::to_value(&state.steps).unwrap_or_default();
    }

    CommandResponse {
        success: true,
        data: Some(data),
        error: None,
    }
}

// ============================================================================
// Public entry points
// ============================================================================

/// Start a new chat turn.  Makes one API call, then either returns the final
/// response or yields tool-call steps for the UI to render before continuing.
pub fn handle_chat(input: ChatInput, config: &PluginConfig) -> CommandResponse {
    let ChatInput {
        message,
        entries,
        managed,
    } = input;

    let (url, api_key, model) = if is_managed_mode(config) {
        let managed = managed.unwrap_or_default();
        let server_url = managed.server_url.unwrap_or_default();
        let auth_token = managed.auth_token.unwrap_or_default();
        let tier = managed.tier.unwrap_or_default();

        if server_url.trim().is_empty() || auth_token.trim().is_empty() || tier.trim().is_empty() {
            return plugin_error(
                "managed_unavailable",
                "Managed mode requires an authenticated Diaryx sync session.",
            );
        }
        if !tier.eq_ignore_ascii_case("plus") {
            return plugin_error(
                "plus_required",
                "Diaryx Plus is required to use managed AI.",
            );
        }

        (
            build_managed_url(&server_url),
            auth_token,
            config
                .managed_model
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(DEFAULT_MANAGED_MODEL)
                .to_string(),
        )
    } else {
        let endpoint = config
            .api_endpoint
            .as_deref()
            .unwrap_or(DEFAULT_BYO_ENDPOINT);
        let api_key = match &config.api_key {
            Some(key) if !key.is_empty() => key.clone(),
            _ => {
                return plugin_error(
                    "api_key_required",
                    "No API key configured. Open Settings → AI to set your API key.",
                );
            }
        };

        (
            build_byo_url(endpoint),
            api_key,
            config
                .model
                .as_deref()
                .unwrap_or(DEFAULT_BYO_MODEL)
                .to_string(),
        )
    };

    ensure_loaded();

    // Build messages
    let mut api_messages: Vec<JsonValue> = Vec::new();

    let default_system = "You are a helpful AI assistant integrated into Diaryx, a personal knowledge management and journaling app. \
         Help the user with their writing, answer questions about their notes, and provide thoughtful suggestions. \
         Be concise and helpful. Format responses in markdown when appropriate.\n\n\
         You have access to tools that let you read files in the user's workspace. \
         Use them when the user asks about their notes, wants summaries, or references content you don't have in context. \
         Call list_files first to discover what's available, then read_file for specific entries.";

    let system_prompt = config
        .system_prompt
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default_system);

    api_messages.push(serde_json::json!({
        "role": "system",
        "content": system_prompt,
    }));

    if !entries.is_empty() {
        let mut context_parts = Vec::new();
        for entry in &entries {
            context_parts.push(format!("## {}\n\n{}", entry.path, entry.content));
        }
        api_messages.push(serde_json::json!({
            "role": "system",
            "content": format!(
                "The user has shared the following notes for context:\n\n{}",
                context_parts.join("\n\n---\n\n")
            ),
        }));
    }

    CONVERSATION.with(|conv| {
        let conv = conv.borrow();
        for msg in &conv.messages {
            api_messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }
    });

    api_messages.push(serde_json::json!({
        "role": "user",
        "content": &message,
    }));

    let mut state = AgentState {
        api_messages,
        user_message: message,
        steps: Vec::new(),
        iterations: 0,
        url,
        model,
        api_key,
    };

    match run_one_iteration(&mut state) {
        IterationResult::ToolCalls => {
            let resp = build_response(&state, None);
            AGENT_STATE.with(|s| *s.borrow_mut() = Some(state));
            resp
        }
        IterationResult::Done(text) => {
            persist_exchange(&state.user_message, &text);
            build_response(&state, Some(&text))
        }
        IterationResult::Error(e) => {
            AGENT_STATE.with(|s| *s.borrow_mut() = None);
            CommandResponse {
                success: false,
                data: None,
                error: Some(e),
            }
        }
    }
}

/// Continue an in-flight agent loop.  Called by the UI after rendering
/// tool-call steps.  Makes one more API call and either yields again
/// or returns the final response.
pub fn chat_continue(_managed: Option<ManagedContext>) -> CommandResponse {
    let mut state = match AGENT_STATE.with(|s| s.borrow_mut().take()) {
        Some(s) => s,
        None => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some("No agent loop in progress".into()),
            };
        }
    };

    if state.iterations >= MAX_AGENT_ITERATIONS {
        return CommandResponse {
            success: false,
            data: None,
            error: Some("Agent reached maximum iterations without a final response".into()),
        };
    }

    match run_one_iteration(&mut state) {
        IterationResult::ToolCalls => {
            let resp = build_response(&state, None);
            AGENT_STATE.with(|s| *s.borrow_mut() = Some(state));
            resp
        }
        IterationResult::Done(text) => {
            persist_exchange(&state.user_message, &text);
            build_response(&state, Some(&text))
        }
        IterationResult::Error(e) => {
            AGENT_STATE.with(|s| *s.borrow_mut() = None);
            CommandResponse {
                success: false,
                data: None,
                error: Some(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatInput, build_byo_url, build_managed_url, handle_chat};
    use crate::PluginConfig;

    #[test]
    fn byo_url_keeps_chat_completions_path_when_already_present() {
        let url = build_byo_url("https://openrouter.ai/api/v1/chat/completions");
        assert_eq!(url, "https://openrouter.ai/api/v1/chat/completions");
    }

    #[test]
    fn byo_url_appends_chat_completions_when_missing() {
        let url = build_byo_url("https://openrouter.ai/api/v1");
        assert_eq!(url, "https://openrouter.ai/api/v1/chat/completions");
    }

    #[test]
    fn managed_url_appends_server_route() {
        let url = build_managed_url("https://sync.diaryx.org/");
        assert_eq!(url, "https://sync.diaryx.org/api/ai/chat/completions");
    }

    #[test]
    fn managed_mode_rejects_missing_context() {
        let config = PluginConfig {
            provider_mode: Some("managed".to_string()),
            ..PluginConfig::default()
        };

        let response = handle_chat(
            ChatInput {
                message: "hello".to_string(),
                entries: Vec::new(),
                managed: None,
            },
            &config,
        );

        assert!(!response.success);
        assert_eq!(
            response
                .data
                .as_ref()
                .and_then(|d| d.get("code"))
                .and_then(|v| v.as_str()),
            Some("managed_unavailable")
        );
    }

    #[test]
    fn managed_mode_rejects_free_tier() {
        let config = PluginConfig {
            provider_mode: Some("managed".to_string()),
            ..PluginConfig::default()
        };

        let response = handle_chat(
            ChatInput {
                message: "hello".to_string(),
                entries: Vec::new(),
                managed: Some(super::ManagedContext {
                    server_url: Some("https://sync.diaryx.org".to_string()),
                    auth_token: Some("token".to_string()),
                    tier: Some("free".to_string()),
                }),
            },
            &config,
        );

        assert!(!response.success);
        assert_eq!(
            response
                .data
                .as_ref()
                .and_then(|d| d.get("code"))
                .and_then(|v| v.as_str()),
            Some("plus_required")
        );
    }
}

/// Return the full conversation history.
pub fn get_history() -> CommandResponse {
    ensure_loaded();
    let messages = CONVERSATION.with(|conv| conv.borrow().messages.clone());
    CommandResponse {
        success: true,
        data: Some(serde_json::to_value(&messages).unwrap_or_default()),
        error: None,
    }
}

/// Delete the active conversation (or reset if none active).
pub fn clear_conversation() -> CommandResponse {
    let conv_id = CONVERSATION.with(|conv| conv.borrow().id.clone());

    if let Some(id) = conv_id {
        // Remove from index
        let mut index = load_index();
        delete_conversation_messages(&id);
        index.conversations.retain(|c| c.id != id);
        index.active_id = None;
        save_index(&index);
    }

    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.id = None;
        conv.messages.clear();
    });
    AGENT_STATE.with(|s| *s.borrow_mut() = None);
    CommandResponse {
        success: true,
        data: None,
        error: None,
    }
}

// ============================================================================
// Multi-conversation commands
// ============================================================================

/// List all conversations with metadata.
pub fn list_conversations() -> CommandResponse {
    ensure_loaded();
    let index = load_index();
    CommandResponse {
        success: true,
        data: Some(serde_json::to_value(&index).unwrap_or_default()),
        error: None,
    }
}

/// Switch to a different conversation by id.
pub fn switch_conversation(id: &str) -> CommandResponse {
    let mut index = load_index();
    let exists = index.conversations.iter().any(|c| c.id == id);

    if !exists {
        return CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Conversation not found: {}", id)),
        };
    }

    index.active_id = Some(id.to_string());
    save_index(&index);

    let messages = load_conversation_messages(id);

    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.id = Some(id.to_string());
        conv.messages = messages.clone();
    });

    CommandResponse {
        success: true,
        data: Some(serde_json::to_value(&messages).unwrap_or_default()),
        error: None,
    }
}

/// Start a new empty conversation (doesn't persist until first message).
pub fn new_conversation() -> CommandResponse {
    // Update index to clear active_id
    let mut index = load_index();
    index.active_id = None;
    save_index(&index);

    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.id = None;
        conv.messages.clear();
    });
    AGENT_STATE.with(|s| *s.borrow_mut() = None);

    CommandResponse {
        success: true,
        data: None,
        error: None,
    }
}

/// Delete a specific conversation by id.
pub fn delete_conversation(id: &str) -> CommandResponse {
    let mut index = load_index();

    if !index.conversations.iter().any(|c| c.id == id) {
        return CommandResponse {
            success: false,
            data: None,
            error: Some(format!("Conversation not found: {}", id)),
        };
    }

    delete_conversation_messages(id);
    index.conversations.retain(|c| c.id != id);

    // If deleting the active conversation, clear active state
    let was_active = index.active_id.as_deref() == Some(id);
    if was_active {
        index.active_id = None;
    }
    save_index(&index);

    if was_active {
        CONVERSATION.with(|conv| {
            let mut conv = conv.borrow_mut();
            conv.id = None;
            conv.messages.clear();
        });
        AGENT_STATE.with(|s| *s.borrow_mut() = None);
    }

    CommandResponse {
        success: true,
        data: None,
        error: None,
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn persist_exchange(user_message: &str, assistant_text: &str) {
    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.messages.push(ChatMessage {
            role: "user".into(),
            content: user_message.into(),
        });
        conv.messages.push(ChatMessage {
            role: "assistant".into(),
            content: assistant_text.into(),
        });

        let mut index = load_index();

        if let Some(ref id) = conv.id {
            // Existing conversation — save messages and update count
            save_conversation_messages(id, &conv.messages);
            if let Some(meta) = index.conversations.iter_mut().find(|c| c.id == *id) {
                meta.message_count = conv.messages.len();
            }
            save_index(&index);
        } else {
            // New conversation — create entry in index
            let id = generate_conversation_id();
            let title = generate_title(user_message);
            let meta = ConversationMeta {
                id: id.clone(),
                title,
                created_at: 0, // no timestamp host fn available
                message_count: conv.messages.len(),
            };

            save_conversation_messages(&id, &conv.messages);

            // Enforce max conversations — remove oldest if at limit
            if index.conversations.len() >= MAX_CONVERSATIONS {
                if let Some(removed) = index.conversations.pop() {
                    delete_conversation_messages(&removed.id);
                }
            }

            index.conversations.insert(0, meta);
            index.active_id = Some(id.clone());
            save_index(&index);

            conv.id = Some(id);
        }
    });
}
