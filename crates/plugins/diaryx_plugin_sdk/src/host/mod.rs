//! Host function wrappers providing typed, safe access to the Diaryx host API.
//!
//! These modules replace the per-plugin `host_bridge.rs` files. Each module
//! corresponds to a capability tier and is gated behind a feature flag.
//!
//! # Capability Tiers
//!
//! | Module     | Feature    | Description                          |
//! |------------|------------|--------------------------------------|
//! | `fs`       | `core`     | File I/O (read, write, list, delete) |
//! | `storage`  | `core`     | Plugin-scoped key-value storage      |
//! | `log`      | `core`     | Structured logging                   |
//! | `time`     | `core`     | Timestamps                           |
//! | `http`     | `http`     | HTTP requests                        |
//! | `secrets`  | `secrets`  | Credential storage                   |
//! | `ws`       | `ws`       | WebSocket bridge                     |
//! | `events`   | `events`   | Event emission                       |
//! | `plugins`  | `plugins`  | Inter-plugin commands                |
//! | `context`  | `context`  | Runtime context                      |
//! | `wasi`     | `wasi`     | WASI module execution                |
//! | `files`    | `files`    | User-provided file requests          |

// ---------------------------------------------------------------------------
// Raw host function declarations
// ---------------------------------------------------------------------------

use extism_pdk::*;

#[host_fn]
extern "ExtismHost" {
    // Core
    pub fn host_log(input: String) -> String;
    pub fn host_read_file(input: String) -> String;
    pub fn host_read_binary(input: String) -> String;
    pub fn host_list_files(input: String) -> String;
    pub fn host_file_exists(input: String) -> String;
    pub fn host_write_file(input: String) -> String;
    pub fn host_write_binary(input: String) -> String;
    pub fn host_delete_file(input: String) -> String;
    pub fn host_storage_get(input: String) -> String;
    pub fn host_storage_set(input: String) -> String;
    pub fn host_get_timestamp(input: String) -> String;
    pub fn host_get_now(input: String) -> String;

    // HTTP
    pub fn host_http_request(input: String) -> String;

    // Secrets
    pub fn host_secret_get(input: String) -> String;
    pub fn host_secret_set(input: String) -> String;
    pub fn host_secret_delete(input: String) -> String;

    // WebSocket
    pub fn host_ws_request(input: String) -> String;

    // Events
    pub fn host_emit_event(input: String) -> String;

    // Inter-plugin
    pub fn host_plugin_command(input: String) -> String;

    // Runtime context
    pub fn host_get_runtime_context(input: String) -> String;

    // WASI
    pub fn host_run_wasi_module(input: String) -> String;

    // User-provided files
    pub fn host_request_file(input: String) -> String;

    // Namespace
    pub fn host_namespace_get_object(input: String) -> String;
    pub fn host_namespace_put_object(input: String) -> String;
    pub fn host_namespace_delete_object(input: String) -> String;
    pub fn host_namespace_list_objects(input: String) -> String;
    pub fn host_namespace_list(input: String) -> String;
    pub fn host_namespace_create(input: String) -> String;
    pub fn host_namespace_sync_audience(input: String) -> String;
    pub fn host_namespace_send_email(input: String) -> String;

    // Hashing
    pub fn host_hash_file(input: String) -> String;

    // Proxy
    pub fn host_proxy_request(input: String) -> String;
}

// WASM-only host functions. These must be in a separate `#[host_fn]` block
// because the proc macro does not propagate per-item `#[cfg]` attributes —
// placing them in the main block generates native symbols that cause linker
// errors when any downstream crate is compiled as a test binary.
#[cfg(target_arch = "wasm32")]
#[host_fn]
extern "ExtismHost" {
    pub fn host_list_dir(input: String) -> String;
    pub fn host_workspace_file_set(input: String) -> String;
    pub fn host_file_metadata(input: String) -> String;
}

// ---------------------------------------------------------------------------
// Module re-exports
// ---------------------------------------------------------------------------

#[cfg(feature = "core")]
pub mod fs;

#[cfg(feature = "core")]
pub mod storage;

#[cfg(feature = "core")]
pub mod log;

#[cfg(feature = "core")]
pub mod time;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "secrets")]
pub mod secrets;

#[cfg(feature = "ws")]
pub mod ws;

#[cfg(feature = "events")]
pub mod events;

#[cfg(feature = "plugins")]
pub mod plugins;

#[cfg(feature = "context")]
pub mod context;

#[cfg(feature = "wasi")]
pub mod wasi;

#[cfg(feature = "files")]
pub mod files;

#[cfg(feature = "namespaces")]
pub mod namespace;

#[cfg(feature = "core")]
pub mod hash;

#[cfg(feature = "proxy")]
pub mod proxy;
