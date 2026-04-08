//! Chat logic: message history, context building, API calls, agent tool-use loop.
//!
//! The agent loop is **UI-driven**: after each batch of tool calls the plugin
//! returns to the iframe with `status: "tool_calls"` and the accumulated steps.
//! The iframe renders those steps then calls `chat_continue` to resume.  This
//! gives the user real-time feedback instead of waiting for the full loop.

use crate::PluginConfig;
use diaryx_plugin_sdk::prelude::*;
use serde_json::Value as JsonValue;

const LEGACY_HISTORY_KEY: &str = "diaryx.ai.history";
const INDEX_KEY: &str = "diaryx.ai.conversations.index";
const CONV_PREFIX: &str = "diaryx.ai.conversations.";
const MAX_HISTORY_MESSAGES: usize = 50;
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
    #[serde(default)]
    pub file_path: Option<String>,
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
    host::storage::get_json::<ConversationIndex>(INDEX_KEY)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn save_index(index: &ConversationIndex) {
    let _ = host::storage::set_json(INDEX_KEY, index);
}

fn conv_storage_key(id: &str) -> String {
    format!("{}{}", CONV_PREFIX, id)
}

fn load_conversation_messages(id: &str) -> Vec<ChatMessage> {
    host::storage::get_json::<Vec<ChatMessage>>(&conv_storage_key(id))
        .ok()
        .flatten()
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
    let trimmed_vec: Vec<_> = trimmed.to_vec();
    let _ = host::storage::set_json(&conv_storage_key(id), &trimmed_vec);
}

fn delete_conversation_messages(id: &str) {
    let _ = host::storage::delete(&conv_storage_key(id));
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
    if host::storage::get(INDEX_KEY).ok().flatten().is_some() {
        return;
    }

    // Check for legacy flat history
    let legacy = host::storage::get_json::<Vec<ChatMessage>>(LEGACY_HISTORY_KEY)
        .ok()
        .flatten()
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
        file_path: None,
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
    fn host_write_file(offset: u64) -> u64;
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

fn call_host_write_file(path: &str, content: &str) {
    let input = serde_json::json!({ "path": path, "content": content }).to_string();
    unsafe {
        let mem = extism_pdk::Memory::from_bytes(input.as_bytes())
            .expect("failed to allocate memory for write_file");
        host_write_file(mem.offset());
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
        error_code: None,
    }
}

fn is_managed_mode(config: &PluginConfig) -> bool {
    config
        .provider_mode
        .as_deref()
        .map(|mode| mode.eq_ignore_ascii_case("managed"))
        .unwrap_or(true)
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

    CommandResponse::ok(data)
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
                "Sign in to Diaryx Sync to use AI. Alternatively, you can use your own API key by switching to \"Bring your own API key\" in Settings \u{2192} AI.",
            );
        }
        if !tier.eq_ignore_ascii_case("plus") {
            return plugin_error(
                "plus_required",
                "Subscribe to Diaryx Plus to use AI. You can also bring your own API key in Settings \u{2192} AI.",
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
            CommandResponse::err(e)
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
            return CommandResponse::err("No agent loop in progress");
        }
    };

    if state.iterations >= MAX_AGENT_ITERATIONS {
        return CommandResponse::err("Agent reached maximum iterations without a final response");
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
            CommandResponse::err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatInput, ChatMessage, build_byo_url, build_managed_url, handle_chat,
        render_conversation_markdown, replace_entry_body,
    };
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

    #[test]
    fn replace_entry_body_preserves_frontmatter() {
        let existing = "---\ntitle: Chat\npart_of: root.md\n---\n\n# Old\n";
        let updated = replace_entry_body(existing, "# New\n\nHello");

        assert!(updated.starts_with("---\ntitle: Chat\npart_of: root.md\n---\n\n"));
        assert!(updated.contains("# New\n\nHello\n"));
        assert!(!updated.contains("# Old"));
    }

    #[test]
    fn render_conversation_markdown_includes_headings() {
        let markdown = render_conversation_markdown(
            "Planning Chat",
            &[
                ChatMessage {
                    role: "user".into(),
                    content: "Help me plan".into(),
                },
                ChatMessage {
                    role: "assistant".into(),
                    content: "Sure".into(),
                },
            ],
        );

        assert!(markdown.contains("# Planning Chat"));
        assert!(markdown.contains("### User"));
        assert!(markdown.contains("### Assistant"));
    }
}

/// Return the full conversation history.
pub fn get_history() -> CommandResponse {
    ensure_loaded();
    let messages = CONVERSATION.with(|conv| conv.borrow().messages.clone());
    CommandResponse::ok(serde_json::to_value(&messages).unwrap_or_default())
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
    CommandResponse::ok_empty()
}

// ============================================================================
// Multi-conversation commands
// ============================================================================

/// List all conversations with metadata.
pub fn list_conversations() -> CommandResponse {
    ensure_loaded();
    let index = load_index();
    CommandResponse::ok(serde_json::to_value(&index).unwrap_or_default())
}

/// Switch to a different conversation by id.
pub fn switch_conversation(id: &str) -> CommandResponse {
    let mut index = load_index();
    let exists = index.conversations.iter().any(|c| c.id == id);

    if !exists {
        return CommandResponse::err(format!("Conversation not found: {}", id));
    }

    index.active_id = Some(id.to_string());
    save_index(&index);

    let messages = load_conversation_messages(id);

    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.id = Some(id.to_string());
        conv.messages = messages.clone();
    });

    CommandResponse::ok(serde_json::to_value(&messages).unwrap_or_default())
}

/// Start a new empty conversation backed by a workspace file.
pub fn new_conversation(title: &str, file_path: Option<String>) -> CommandResponse {
    ensure_loaded();

    let trimmed_title = title.trim();
    if trimmed_title.is_empty() {
        return CommandResponse::err("A conversation title is required");
    }

    let id = generate_conversation_id();
    let meta = ConversationMeta {
        id: id.clone(),
        title: trimmed_title.to_string(),
        created_at: 0,
        message_count: 0,
        file_path,
    };

    let mut index = load_index();
    index.conversations.insert(0, meta.clone());
    index.active_id = Some(id.clone());
    save_index(&index);

    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.id = Some(id);
        conv.messages.clear();
        conv.loaded = true;
    });
    AGENT_STATE.with(|s| *s.borrow_mut() = None);

    CommandResponse::ok(serde_json::to_value(&meta).unwrap_or_default())
}

/// Delete a specific conversation by id.
pub fn delete_conversation(id: &str) -> CommandResponse {
    let mut index = load_index();

    if !index.conversations.iter().any(|c| c.id == id) {
        return CommandResponse::err(format!("Conversation not found: {}", id));
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

    CommandResponse::ok_empty()
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
                sync_conversation_file(meta, &conv.messages);
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
                file_path: None,
            };

            save_conversation_messages(&id, &conv.messages);

            index.conversations.insert(0, meta);
            index.active_id = Some(id.clone());
            save_index(&index);

            conv.id = Some(id);
        }
    });
}

fn sync_conversation_file(meta: &ConversationMeta, messages: &[ChatMessage]) {
    let Some(path) = meta.file_path.as_deref() else {
        return;
    };

    let existing = call_host_read_file(path);
    if existing.is_empty() {
        return;
    }

    let body = render_conversation_markdown(&meta.title, messages);
    let merged = replace_entry_body(&existing, &body);
    call_host_write_file(path, &merged);
}

fn render_conversation_markdown(title: &str, messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(title.trim());
    out.push_str("\n\n");
    out.push_str("_This conversation is managed by the AI Assistant plugin._\n\n");
    out.push_str("## Conversation\n\n");

    for (idx, message) in messages.iter().enumerate() {
        out.push_str("### ");
        out.push_str(role_heading(&message.role));
        out.push_str("\n\n");
        out.push_str(message.content.trim_end());
        out.push('\n');
        if idx + 1 < messages.len() {
            out.push_str("\n---\n\n");
        }
    }

    out
}

fn role_heading(role: &str) -> &str {
    match role {
        "assistant" => "Assistant",
        "user" => "User",
        _ => "Message",
    }
}

fn replace_entry_body(existing: &str, new_body: &str) -> String {
    let (frontmatter, _) = split_frontmatter(existing);
    let body = format!("{}\n", new_body.trim_end());
    if frontmatter.is_empty() {
        return body;
    }

    let mut merged = frontmatter;
    if !merged.ends_with("\n\n") {
        if !merged.ends_with('\n') {
            merged.push('\n');
        }
        merged.push('\n');
    }
    merged.push_str(&body);
    merged
}

fn split_frontmatter(content: &str) -> (String, &str) {
    let line_ending = if content.starts_with("---\r\n") {
        "\r\n"
    } else if content.starts_with("---\n") {
        "\n"
    } else {
        return (String::new(), content);
    };

    let start = 3 + line_ending.len();
    let marker = format!("{}---", line_ending);
    let Some(rel_end) = content[start..].find(&marker) else {
        return (String::new(), content);
    };

    let mut end = start + rel_end + marker.len();
    while content[end..].starts_with(line_ending) {
        end += line_ending.len();
    }

    (content[..end].to_string(), &content[end..])
}
