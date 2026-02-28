//! Host functions exposed to guest WASM plugins.
//!
//! These functions give guest plugins controlled, sandboxed access to the
//! Diaryx environment. They are registered with the Extism plugin via
//! [`PluginBuilder`](extism::PluginBuilder).

use std::path::Path;
use std::sync::Arc;

use diaryx_core::fs::AsyncFileSystem;
use extism::{CurrentPlugin, Error as ExtismError, UserData, Val, ValType};

/// Context shared with host functions via Extism's `UserData` mechanism.
///
/// Provides guest plugins with controlled access to the workspace filesystem.
pub struct HostContext {
    /// Type-erased async filesystem for workspace file access.
    pub fs: Arc<dyn AsyncFileSystem>,
}

// SAFETY: HostContext only contains Arc<dyn AsyncFileSystem> which requires
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
            user_data,
            host_file_exists,
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
