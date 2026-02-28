//! Host functions exposed to guest WASM plugins.
//!
//! These functions give guest plugins controlled, sandboxed access to the
//! Diaryx environment. They are registered with the Extism plugin via
//! [`PluginBuilder`](extism::PluginBuilder).

use std::path::Path;
use std::sync::Arc;

use diaryx_core::fs::AsyncFileSystem;
use extism::{CurrentPlugin, Error as ExtismError, UserData, Val, ValType};

/// Trait for persisting plugin state (CRDT snapshots, config, etc.).
///
/// Implementations might use SQLite on native or IndexedDB on web.
pub trait PluginStorage: Send + Sync {
    /// Load a value by key.
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    /// Store a value by key.
    fn set(&self, key: &str, data: &[u8]);
    /// Delete a value by key.
    fn delete(&self, key: &str);
}

/// Trait for emitting events from plugins to the host application.
pub trait EventEmitter: Send + Sync {
    /// Emit an event (JSON payload) to the host.
    fn emit(&self, event_json: &str);
}

/// No-op implementation of [`PluginStorage`] for plugins that don't need persistence.
pub struct NoopStorage;

impl PluginStorage for NoopStorage {
    fn get(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }
    fn set(&self, _key: &str, _data: &[u8]) {}
    fn delete(&self, _key: &str) {}
}

/// No-op implementation of [`EventEmitter`] for plugins that don't emit events.
pub struct NoopEventEmitter;

impl EventEmitter for NoopEventEmitter {
    fn emit(&self, _event_json: &str) {}
}

/// Context shared with host functions via Extism's `UserData` mechanism.
///
/// Provides guest plugins with controlled access to the workspace filesystem,
/// persistent storage, and event dispatch.
pub struct HostContext {
    /// Type-erased async filesystem for workspace file access.
    pub fs: Arc<dyn AsyncFileSystem>,
    /// Persistent storage for plugin state (CRDT snapshots, etc.).
    pub storage: Arc<dyn PluginStorage>,
    /// Event emitter for sync events.
    pub event_emitter: Arc<dyn EventEmitter>,
}

impl HostContext {
    /// Create a context with just a filesystem (backwards compatible).
    pub fn with_fs(fs: Arc<dyn AsyncFileSystem>) -> Self {
        Self {
            fs,
            storage: Arc::new(NoopStorage),
            event_emitter: Arc::new(NoopEventEmitter),
        }
    }
}

// SAFETY: HostContext only contains Arc<dyn Trait> values which require
// Send + Sync on native targets.
unsafe impl Send for HostContext {}
unsafe impl Sync for HostContext {}

/// Register all host functions on an Extism `PluginBuilder`.
///
/// The builder is consumed and returned with host functions attached.
pub fn register_host_functions(
    builder: extism::PluginBuilder<'_>,
    user_data: UserData<HostContext>,
) -> extism::PluginBuilder<'_> {
    builder
        .with_function(
            "host_log",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_log,
        )
        .with_function(
            "host_read_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_read_file,
        )
        .with_function(
            "host_list_files",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_list_files,
        )
        .with_function(
            "host_file_exists",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_file_exists,
        )
        .with_function(
            "host_write_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_write_file,
        )
        .with_function(
            "host_write_binary",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_write_binary,
        )
        .with_function(
            "host_emit_event",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_emit_event,
        )
        .with_function(
            "host_storage_get",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_storage_get,
        )
        .with_function(
            "host_storage_set",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_storage_set,
        )
        .with_function(
            "host_get_timestamp",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_get_timestamp,
        )
        .with_function(
            "host_http_request",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_http_request,
        )
        .with_function(
            "host_ws_request",
            [ValType::I64],
            [ValType::I64],
            user_data,
            host_ws_request,
        )
}

/// Host function: `host_log(input: {level, message}) -> ""`
///
/// Logs a message via the `log` crate at the specified level.
fn host_log(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct LogInput {
        level: String,
        message: String,
    }

    let parsed: LogInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_log: invalid input: {e}")))?;

    match parsed.level.as_str() {
        "error" => log::error!("[extism-plugin] {}", parsed.message),
        "warn" => log::warn!("[extism-plugin] {}", parsed.message),
        "info" => log::info!("[extism-plugin] {}", parsed.message),
        "debug" => log::debug!("[extism-plugin] {}", parsed.message),
        _ => log::trace!("[extism-plugin] {}", parsed.message),
    }

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_read_file(input: {path}) -> file content string`
///
/// Reads a workspace file and returns its content.
fn host_read_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ReadInput {
        path: String,
    }

    let parsed: ReadInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_read_file: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let content = futures_lite::future::block_on(ctx.fs.read_to_string(Path::new(&parsed.path)))
        .map_err(|e| ExtismError::msg(format!("host_read_file: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], content.as_str())?;
    Ok(())
}

/// Host function: `host_list_files(input: {prefix}) -> string[] JSON`
///
/// Lists files under a given prefix in the workspace.
fn host_list_files(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ListInput {
        prefix: String,
    }

    let parsed: ListInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_list_files: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    let files =
        futures_lite::future::block_on(ctx.fs.list_all_files_recursive(Path::new(&parsed.prefix)))
            .map_err(|e| ExtismError::msg(format!("host_list_files: {e}")))?;

    let file_strings: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let json = serde_json::to_string(&file_strings)
        .map_err(|e| ExtismError::msg(format!("host_list_files: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_file_exists(input: {path}) -> bool JSON`
///
/// Checks if a file exists in the workspace.
fn host_file_exists(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ExistsInput {
        path: String,
    }

    let parsed: ExistsInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_file_exists: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    // exists() returns bool directly (not Result<bool>)
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&parsed.path)));

    let json = serde_json::to_string(&exists)
        .map_err(|e| ExtismError::msg(format!("host_file_exists: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_write_file(input: {path, content}) -> ""`
///
/// Writes a text file to the workspace.
fn host_write_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct WriteInput {
        path: String,
        content: String,
    }

    let parsed: WriteInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_write_file: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    futures_lite::future::block_on(ctx.fs.write_file(Path::new(&parsed.path), &parsed.content))
        .map_err(|e| ExtismError::msg(format!("host_write_file: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_write_binary(input: {path, content}) -> ""`
///
/// Writes binary content (base64-encoded) to a file.
fn host_write_binary(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct WriteBinaryInput {
        path: String,
        content: String, // base64-encoded
    }

    let parsed: WriteBinaryInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_write_binary: invalid input: {e}")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed.content)
        .map_err(|e| ExtismError::msg(format!("host_write_binary: base64 decode: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    futures_lite::future::block_on(ctx.fs.write_binary(Path::new(&parsed.path), &bytes))
        .map_err(|e| ExtismError::msg(format!("host_write_binary: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_emit_event(input: event_json) -> ""`
///
/// Emits a sync event to the host application.
fn host_emit_event(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let event_json: String = plugin.memory_get_val(&inputs[0])?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    ctx.event_emitter.emit(&event_json);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_storage_get(input: {key}) -> {data: base64} or ""`
///
/// Loads persisted state by key.
fn host_storage_get(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct StorageGetInput {
        key: String,
    }

    let parsed: StorageGetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_storage_get: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();

    let result = match ctx.storage.get(&parsed.key) {
        Some(data) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            serde_json::json!({ "data": encoded }).to_string()
        }
        None => String::new(),
    };

    plugin.memory_set_val(&mut outputs[0], result.as_str())?;
    Ok(())
}

/// Host function: `host_storage_set(input: {key, data}) -> ""`
///
/// Persists state by key (data is base64-encoded).
fn host_storage_set(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct StorageSetInput {
        key: String,
        data: String, // base64-encoded
    }

    let parsed: StorageSetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_storage_set: invalid input: {e}")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed.data)
        .map_err(|e| ExtismError::msg(format!("host_storage_set: base64 decode: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    ctx.storage.set(&parsed.key, &bytes);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_get_timestamp(input: "") -> timestamp_ms string`
///
/// Returns the current timestamp in milliseconds since epoch.
fn host_get_timestamp(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    plugin.memory_set_val(&mut outputs[0], now.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_ws_request(input: json) -> string`
///
/// Forward-compatible bridge for plugin-managed websocket ownership.
/// Current host implementations keep socket lifecycle in JS/Rust transports,
/// so this is intentionally a no-op stub.
fn host_ws_request(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_http_request(input: {url, method, headers, body?}) -> {status, headers, body}`
///
/// Performs an HTTP request and returns the response. Only available when
/// the `http` feature is enabled (native builds). On WASM the browser
/// host functions provide the equivalent via `fetch()`.
#[cfg(feature = "http")]
fn host_http_request(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct HttpInput {
        url: String,
        method: String,
        headers: std::collections::HashMap<String, String>,
        body: Option<String>,
    }

    #[derive(serde::Serialize)]
    struct HttpOutput {
        status: u16,
        headers: std::collections::HashMap<String, String>,
        body: String,
    }

    let parsed: HttpInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_http_request: invalid input: {e}")))?;

    let mut request = ureq::request(&parsed.method, &parsed.url);
    for (key, value) in &parsed.headers {
        request = request.set(key, value);
    }

    let response = if let Some(body) = &parsed.body {
        request
            .send_string(&body)
            .map_err(|e| ExtismError::msg(format!("host_http_request: {e}")))?
    } else {
        request
            .call()
            .map_err(|e| ExtismError::msg(format!("host_http_request: {e}")))?
    };

    let status = response.status();
    let mut resp_headers = std::collections::HashMap::new();
    for name in response.headers_names() {
        if let Some(value) = response.header(&name) {
            resp_headers.insert(name, value.to_string());
        }
    }
    let body = response
        .into_string()
        .map_err(|e| ExtismError::msg(format!("host_http_request: read body: {e}")))?;

    let output = HttpOutput {
        status,
        headers: resp_headers,
        body,
    };

    let json = serde_json::to_string(&output)
        .map_err(|e| ExtismError::msg(format!("host_http_request: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Stub for `host_http_request` when the `http` feature is not enabled.
#[cfg(not(feature = "http"))]
fn host_http_request(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let error = serde_json::json!({
        "status": 0,
        "headers": {},
        "body": "host_http_request: http feature not enabled"
    });
    plugin.memory_set_val(&mut outputs[0], error.to_string().as_str())?;
    Ok(())
}
