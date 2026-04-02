//! Standalone WASI module runner.
//!
//! Allows Extism guest plugins to execute arbitrary WASI programs (e.g. pandoc,
//! typst) by delegating to the host runtime. The host loads the WASI module
//! from plugin storage, sets up a sandboxed filesystem, and captures
//! stdout/stderr.
//!
//! This module is gated behind the `wasi-runner` feature.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Request to run a WASI module.
#[derive(Debug, Deserialize)]
pub struct WasiRunRequest {
    /// Storage key holding the WASM bytes (retrieved via `PluginStorage::get`).
    pub module_key: String,
    /// CLI arguments (argv) passed to the WASI program.
    pub args: Vec<String>,
    /// Optional stdin data (base64-encoded).
    #[serde(default)]
    pub stdin: Option<String>,
    /// Virtual filesystem files to make available (path → base64 content).
    #[serde(default)]
    pub files: Option<HashMap<String, String>>,
    /// Paths of output files to capture after execution (path → base64 content).
    #[serde(default)]
    pub output_files: Option<Vec<String>>,
}

/// Result of running a WASI module.
#[derive(Debug, Serialize)]
pub struct WasiRunResult {
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Captured stdout (base64-encoded).
    pub stdout: String,
    /// Captured stderr (UTF-8 text).
    pub stderr: String,
    /// Captured output files (path → base64 content).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<HashMap<String, String>>,
}

/// Run a WASI module with the given bytes, arguments, and virtual filesystem.
///
/// This creates a temporary directory, writes input files, runs the module's
/// `_start` export, captures stdout/stderr, and reads back output files.
pub fn run_wasi_module(
    wasm_bytes: &[u8],
    args: &[String],
    stdin: Option<&[u8]>,
    files: Option<&HashMap<String, Vec<u8>>>,
    output_files: Option<&[String]>,
) -> Result<WasiRunResult, String> {
    use base64::Engine;
    use wasi_common::pipe::{ReadPipe, WritePipe};
    use wasi_common::sync::{Dir, WasiCtxBuilder, ambient_authority};
    use wasmtime::{Engine as WasmEngine, Linker, Module, Store};

    // Create a temp directory for the virtual filesystem
    let temp_dir =
        tempfile::tempdir().map_err(|e| format!("Failed to create temp directory: {e}"))?;
    let work_dir = temp_dir.path();

    // Write input files to the temp directory
    if let Some(input_files) = files {
        for (path, data) in input_files {
            let full_path = work_dir.join(path.trim_start_matches('/'));
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create dir {}: {e}", parent.display()))?;
            }
            std::fs::write(&full_path, data)
                .map_err(|e| format!("Failed to write {}: {e}", full_path.display()))?;
        }
    }

    // Build WASI context
    let mut ctx_builder = WasiCtxBuilder::new();

    // Set arguments: argv[0] is the program name, then user args
    let mut full_args: Vec<String> = vec!["program".to_string()];
    full_args.extend_from_slice(args);
    ctx_builder
        .args(&full_args)
        .map_err(|e| format!("Failed to set args: {e}"))?;

    // Set stdin
    if let Some(stdin_data) = stdin {
        ctx_builder.stdin(Box::new(ReadPipe::from(stdin_data.to_vec())));
    }

    // Capture stdout/stderr
    let stdout_pipe = WritePipe::new_in_memory();
    let stderr_pipe = WritePipe::new_in_memory();
    ctx_builder.stdout(Box::new(stdout_pipe.clone()));
    ctx_builder.stderr(Box::new(stderr_pipe.clone()));

    // Preopen the work directory as "." — this is the only directory the WASI
    // module can access. We do NOT preopen "/" to prevent escape from the sandbox.
    let preopen_dir = Dir::open_ambient_dir(work_dir, ambient_authority())
        .map_err(|e| format!("Failed to open work dir: {e}"))?;
    ctx_builder
        .preopened_dir(preopen_dir, ".")
        .map_err(|e| format!("Failed to preopen dir: {e}"))?;

    let wasi_ctx = ctx_builder.build();

    // Compile and instantiate
    let engine = if let Some(config) = crate::platform_wasmtime_config() {
        WasmEngine::new(&config).map_err(|e| format!("Failed to create wasmtime engine: {e}"))?
    } else {
        WasmEngine::default()
    };
    let module = Module::new(&engine, wasm_bytes)
        .map_err(|e| format!("Failed to compile WASI module: {e}"))?;

    let mut linker = Linker::new(&engine);
    wasi_common::sync::add_to_linker(&mut linker, |s| s)
        .map_err(|e| format!("Failed to add WASI to linker: {e}"))?;

    let mut store = Store::new(&engine, wasi_ctx);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("Failed to instantiate WASI module: {e}"))?;

    // Call _start
    let start_fn = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|e| format!("WASI module has no _start export: {e}"))?;

    let exit_code = match start_fn.call(&mut store, ()) {
        Ok(()) => 0,
        Err(e) => {
            // Check for WASI proc_exit (normal termination)
            if let Some(exit) = e.downcast_ref::<wasi_common::I32Exit>() {
                exit.0
            } else {
                // Extract stdout/stderr even on error for diagnostics
                let stderr_bytes = stderr_pipe
                    .try_into_inner()
                    .map(|c| c.into_inner())
                    .unwrap_or_default();
                let stderr_text = String::from_utf8_lossy(&stderr_bytes);
                return Err(format!("WASI module trapped: {e}\nstderr: {stderr_text}"));
            }
        }
    };

    // Drop the store so pipes can be consumed
    drop(store);

    // Read captured stdout
    let stdout_bytes = stdout_pipe
        .try_into_inner()
        .map(|c| c.into_inner())
        .unwrap_or_default();
    let stdout_b64 = base64::engine::general_purpose::STANDARD.encode(&stdout_bytes);

    // Read captured stderr
    let stderr_bytes = stderr_pipe
        .try_into_inner()
        .map(|c| c.into_inner())
        .unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes).to_string();

    // Read output files
    let captured_files = if let Some(paths) = output_files {
        let mut result = HashMap::new();
        for path in paths {
            let full_path = work_dir.join(path.trim_start_matches('/'));
            if full_path.exists() {
                match std::fs::read(&full_path) {
                    Ok(data) => {
                        result.insert(
                            path.clone(),
                            base64::engine::general_purpose::STANDARD.encode(&data),
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[wasi_runner] Failed to read output file {}: {e}",
                            full_path.display()
                        );
                    }
                }
            }
        }
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    } else {
        None
    };

    Ok(WasiRunResult {
        exit_code,
        stdout: stdout_b64,
        stderr: stderr_text,
        files: captured_files,
    })
}
