//! Chat logic: message history, context building, API calls, agent tool-use loop.
//!
//! The agent loop is **UI-driven**: after each batch of tool calls the plugin
//! returns to the iframe with `status: "tool_calls"` and the accumulated steps.
//! The iframe renders those steps then calls `chat_continue` to resume.  This
//! gives the user real-time feedback instead of waiting for the full loop.

use crate::{CommandResponse, PluginConfig, storage_get, storage_set};
use serde_json::Value as JsonValue;

const HISTORY_KEY: &str = "diaryx.ai.history";
const MAX_HISTORY_MESSAGES: usize = 50;
const MAX_AGENT_ITERATIONS: usize = 10;
const MAX_TOOL_RESULT_BYTES: usize = 8192;

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
}

/// Entry context attached to a chat request.
#[derive(serde::Deserialize)]
pub struct EntryContext {
    pub path: String,
    pub content: String,
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
    pub messages: Vec<ChatMessage>,
    pub loaded: bool,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
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
// History persistence
// ============================================================================

fn load_history() -> Vec<ChatMessage> {
    storage_get(HISTORY_KEY)
        .and_then(|bytes| serde_json::from_slice::<Vec<ChatMessage>>(&bytes).ok())
        .unwrap_or_default()
}

fn save_history(messages: &[ChatMessage]) {
    let trimmed: &[ChatMessage] = if messages.len() > MAX_HISTORY_MESSAGES {
        let excess = messages.len() - MAX_HISTORY_MESSAGES;
        let start = if excess % 2 == 0 { excess } else { excess + 1 };
        &messages[start..]
    } else {
        messages
    };
    let bytes = serde_json::to_vec(trimmed).unwrap_or_default();
    storage_set(HISTORY_KEY, &bytes);
}

fn ensure_loaded() {
    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        if !conv.loaded {
            conv.messages = load_history();
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
    let endpoint = config
        .api_endpoint
        .as_deref()
        .unwrap_or("https://openrouter.ai/api/v1/chat/completions");
    let api_key = match &config.api_key {
        Some(key) if !key.is_empty() => key.clone(),
        _ => {
            return CommandResponse {
                success: false,
                data: None,
                error: Some(
                    "No API key configured. Open Settings → AI to set your API key.".into(),
                ),
            };
        }
    };
    let model = config
        .model
        .as_deref()
        .unwrap_or("anthropic/claude-sonnet-4-6")
        .to_string();

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

    if !input.entries.is_empty() {
        let mut context_parts = Vec::new();
        for entry in &input.entries {
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
        "content": &input.message,
    }));

    let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));

    let mut state = AgentState {
        api_messages,
        user_message: input.message,
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
pub fn chat_continue() -> CommandResponse {
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

/// Clear conversation history.
pub fn clear_conversation() -> CommandResponse {
    CONVERSATION.with(|conv| {
        let mut conv = conv.borrow_mut();
        conv.messages.clear();
        save_history(&conv.messages);
    });
    AGENT_STATE.with(|s| *s.borrow_mut() = None);
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
        save_history(&conv.messages);
    });
}
