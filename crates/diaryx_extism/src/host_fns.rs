//! Host functions exposed to guest WASM plugins.
//!
//! These functions give guest plugins controlled, sandboxed access to the
//! Diaryx environment. They are registered with the Extism plugin via
//! [`PluginBuilder`](extism::PluginBuilder).

use std::path::Path;
use std::sync::Arc;

use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::plugin::permissions::PermissionType;
use extism::{CurrentPlugin, Error as ExtismError, UserData, Val, ValType};

use crate::permission_checker::DenyAllPermissionChecker;

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

/// Trait for providing user-selected files to plugins.
///
/// On CLI, files come from command-line arguments (paths read into memory).
/// On browser, files come from File input elements or drag-and-drop.
/// Plugins request files by key name (e.g. "source_file", "dayone_export").
pub trait FileProvider: Send + Sync {
    /// Get file bytes by key name. Returns `None` if no file is available for that key.
    fn get_file(&self, key: &str) -> Option<Vec<u8>>;
}

/// No-op implementation of [`FileProvider`] — always returns `None`.
pub struct NoopFileProvider;

impl FileProvider for NoopFileProvider {
    fn get_file(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }
}

/// [`FileProvider`] backed by a pre-populated map.
///
/// Used by the CLI to pass files read from command-line arguments.
pub struct MapFileProvider {
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl MapFileProvider {
    pub fn new(files: std::collections::HashMap<String, Vec<u8>>) -> Self {
        Self { files }
    }
}

impl FileProvider for MapFileProvider {
    fn get_file(&self, key: &str) -> Option<Vec<u8>> {
        self.files.get(key).cloned()
    }
}

/// No-op implementation of [`EventEmitter`] for plugins that don't emit events.
pub struct NoopEventEmitter;

impl EventEmitter for NoopEventEmitter {
    fn emit(&self, _event_json: &str) {}
}

/// Trait for checking plugin permissions before allowing host function calls.
///
/// Implementations may check static config, prompt the user, or consult
/// a session-level cache.
pub trait PermissionChecker: Send + Sync {
    /// Check if a plugin has permission for an action.
    ///
    /// Returns `Ok(())` if allowed, `Err(message)` if denied.
    /// The `target` is context-dependent: file path, URL, command name, etc.
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String>;
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
    /// Which plugin this context belongs to.
    pub plugin_id: String,
    /// Permission checker (None = deny all).
    pub permission_checker: Option<Arc<dyn PermissionChecker>>,
    /// Provider of user-selected files (e.g. from CLI args or browser file picker).
    pub file_provider: Arc<dyn FileProvider>,
}

impl HostContext {
    /// Create a context with just a filesystem (backwards compatible).
    pub fn with_fs(fs: Arc<dyn AsyncFileSystem>) -> Self {
        Self {
            fs,
            storage: Arc::new(NoopStorage),
            event_emitter: Arc::new(NoopEventEmitter),
            plugin_id: String::new(),
            permission_checker: Some(Arc::new(DenyAllPermissionChecker)),
            file_provider: Arc::new(NoopFileProvider),
        }
    }

    /// Check a permission, returning an Extism error if denied.
    fn check_perm(&self, perm: PermissionType, target: &str) -> Result<(), ExtismError> {
        if let Some(checker) = &self.permission_checker {
            checker
                .check_permission(&self.plugin_id, perm, target)
                .map_err(|msg| ExtismError::msg(msg))
        } else {
            Err(ExtismError::msg(
                "Permission checker is not configured for this plugin host context",
            ))
        }
    }

    fn storage_key(&self, key: &str) -> String {
        if self.plugin_id.is_empty() {
            key.to_string()
        } else {
            format!("{}:{}", self.plugin_id, key)
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
            "host_delete_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_delete_file,
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
            "host_run_wasi_module",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_run_wasi_module,
        )
        .with_function(
            "host_request_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_request_file,
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
    ctx.check_perm(PermissionType::ReadFiles, &parsed.path)?;
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
    ctx.check_perm(PermissionType::ReadFiles, &parsed.prefix)?;
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
    ctx.check_perm(PermissionType::ReadFiles, &parsed.path)?;
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
    // Check edit or create based on whether the file exists
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&parsed.path)));
    let perm = if exists {
        PermissionType::EditFiles
    } else {
        PermissionType::CreateFiles
    };
    ctx.check_perm(perm, &parsed.path)?;
    futures_lite::future::block_on(ctx.fs.write_file(Path::new(&parsed.path), &parsed.content))
        .map_err(|e| ExtismError::msg(format!("host_write_file: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_delete_file(input: {path}) -> ""`
///
/// Deletes a file from the workspace.
fn host_delete_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct DeleteInput {
        path: String,
    }

    let parsed: DeleteInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_delete_file: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    ctx.check_perm(PermissionType::DeleteFiles, &parsed.path)?;
    futures_lite::future::block_on(ctx.fs.delete_file(Path::new(&parsed.path)))
        .map_err(|e| ExtismError::msg(format!("host_delete_file: {e}")))?;

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
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&parsed.path)));
    let perm = if exists {
        PermissionType::EditFiles
    } else {
        PermissionType::CreateFiles
    };
    ctx.check_perm(perm, &parsed.path)?;
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
    ctx.check_perm(PermissionType::PluginStorage, &parsed.key)?;
    let storage_key = ctx.storage_key(&parsed.key);

    let result = match ctx.storage.get(&storage_key) {
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
    ctx.check_perm(PermissionType::PluginStorage, &parsed.key)?;
    let storage_key = ctx.storage_key(&parsed.key);
    ctx.storage.set(&storage_key, &bytes);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_run_wasi_module(input: WasiRunRequest) -> WasiRunResult`
///
/// Runs a WASI module stored in plugin storage. The guest provides a storage
/// key, CLI arguments, optional stdin, virtual filesystem files, and a list
/// of output files to capture. Only available when the `wasi-runner` feature
/// is enabled.
#[cfg(feature = "wasi-runner")]
fn host_run_wasi_module(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;
    let request: crate::wasi_runner::WasiRunRequest = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_run_wasi_module: invalid input: {e}")))?;

    // Load the WASM module bytes from plugin storage
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();
    ctx.check_perm(PermissionType::PluginStorage, &request.module_key)?;
    let storage_key = ctx.storage_key(&request.module_key);
    let wasm_bytes = ctx.storage.get(&storage_key).ok_or_else(|| {
        ExtismError::msg(format!(
            "host_run_wasi_module: module not found in storage: {}",
            request.module_key
        ))
    })?;
    drop(ctx);

    // Decode input files from base64
    let decoded_files = if let Some(ref files) = request.files {
        let mut map = std::collections::HashMap::new();
        for (path, b64) in files {
            let data = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| {
                    ExtismError::msg(format!(
                        "host_run_wasi_module: base64 decode for {path}: {e}"
                    ))
                })?;
            map.insert(path.clone(), data);
        }
        Some(map)
    } else {
        None
    };

    // Decode stdin from base64
    let stdin_bytes = if let Some(ref b64) = request.stdin {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| {
                    ExtismError::msg(format!("host_run_wasi_module: stdin base64 decode: {e}"))
                })?,
        )
    } else {
        None
    };

    // Run the module
    let result = crate::wasi_runner::run_wasi_module(
        &wasm_bytes,
        &request.args,
        stdin_bytes.as_deref(),
        decoded_files.as_ref(),
        request.output_files.as_deref(),
    )
    .map_err(|e| ExtismError::msg(format!("host_run_wasi_module: {e}")))?;

    let json = serde_json::to_string(&result)
        .map_err(|e| ExtismError::msg(format!("host_run_wasi_module: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Stub for `host_run_wasi_module` when the `wasi-runner` feature is not enabled.
#[cfg(not(feature = "wasi-runner"))]
fn host_run_wasi_module(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let error = serde_json::json!({
        "exit_code": -1,
        "stdout": "",
        "stderr": "host_run_wasi_module: wasi-runner feature not enabled"
    });
    plugin.memory_set_val(&mut outputs[0], error.to_string().as_str())?;
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

/// Host function: `host_request_file(input: {key}) -> {data: base64} or ""`
///
/// Requests a user-provided file by key name. The host decides where the
/// file comes from (CLI: read from path in command args; browser: File picker).
/// Returns base64-encoded bytes wrapped in JSON, or empty string if unavailable.
fn host_request_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct RequestFileInput {
        key: String,
    }

    let parsed: RequestFileInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_request_file: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap();

    let result = match ctx.file_provider.get_file(&parsed.key) {
        Some(data) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            serde_json::json!({ "data": encoded }).to_string()
        }
        None => String::new(),
    };

    plugin.memory_set_val(&mut outputs[0], result.as_str())?;
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
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine as _;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct HttpInput {
        url: String,
        method: String,
        headers: std::collections::HashMap<String, String>,
        body: Option<String>,
        /// Base64-encoded binary body. Takes priority over `body` when present.
        body_base64: Option<String>,
    }

    #[derive(serde::Serialize)]
    struct HttpOutput {
        status: u16,
        headers: std::collections::HashMap<String, String>,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body_base64: Option<String>,
    }

    let parsed: HttpInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_http_request: invalid input: {e}")))?;

    {
        let ctx = user_data.get()?;
        let ctx = ctx.lock().unwrap();
        ctx.check_perm(PermissionType::HttpRequests, &parsed.url)?;
    }

    let mut request = ureq::request(&parsed.method, &parsed.url);
    for (key, value) in &parsed.headers {
        request = request.set(key, value);
    }

    let response = if let Some(b64) = &parsed.body_base64 {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| ExtismError::msg(format!("host_http_request: base64 decode: {e}")))?;
        request
            .send_bytes(&bytes)
            .map_err(|e| ExtismError::msg(format!("host_http_request: {e}")))?
    } else if let Some(body) = &parsed.body {
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
    let mut reader = response.into_reader();
    let mut body_bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut body_bytes)
        .map_err(|e| ExtismError::msg(format!("host_http_request: read body: {e}")))?;
    let body = String::from_utf8_lossy(&body_bytes).to_string();
    let body_base64 = Some(base64::engine::general_purpose::STANDARD.encode(&body_bytes));

    let output = HttpOutput {
        status,
        headers: resp_headers,
        body,
        body_base64,
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
