//! WASI module execution inside the plugin sandbox.
//!
//! Requires the `wasi` feature. Allows plugins to run WASI modules stored
//! in plugin storage (e.g., for format conversion, code execution).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::*;

/// Request to execute a WASI module.
#[derive(Debug, Clone, Serialize)]
pub struct WasiRunRequest {
    /// Storage key where the `.wasm` binary is stored.
    pub module_key: String,
    /// Command-line arguments to pass to the module.
    pub args: Vec<String>,
    /// Base64-encoded stdin data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    /// Virtual filesystem: path -> base64-encoded content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<HashMap<String, String>>,
    /// Paths to capture after execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_files: Option<Vec<String>>,
}

/// Result from executing a WASI module.
#[derive(Debug, Clone, Deserialize)]
pub struct WasiRunResult {
    /// Process exit code.
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Captured output files: path -> base64-encoded content.
    pub files: Option<HashMap<String, String>>,
}

/// Run a WASI module stored in plugin storage.
pub fn run(request: &WasiRunRequest) -> Result<WasiRunResult, String> {
    let input = serde_json::to_string(request)
        .map_err(|e| format!("Failed to serialize WASI request: {e}"))?;
    let result = unsafe { host_run_wasi_module(input) }
        .map_err(|e| format!("host_run_wasi_module failed: {e}"))?;
    serde_json::from_str(&result).map_err(|e| format!("Failed to parse WASI result: {e}"))
}
