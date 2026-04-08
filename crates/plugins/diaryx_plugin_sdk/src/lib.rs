//! # Diaryx Plugin SDK
//!
//! The official SDK for building [Diaryx](https://diaryx.com) Extism WASM plugins.
//!
//! This crate replaces the per-plugin boilerplate: protocol types, host function
//! wrappers, getrandom shims, and state management helpers. Depend on this
//! instead of copy-pasting `host_bridge.rs` and protocol structs.
//!
//! ## Quick Start
//!
//! ```toml
//! [package]
//! name = "diaryx_myplugin_extism"
//! version = "0.1.0"
//! edition = "2024"
//!
//! [lib]
//! crate-type = ["cdylib"]
//!
//! [dependencies]
//! diaryx_plugin_sdk = { version = "0.1", features = ["full"] }
//! extism-pdk = "1.4"
//! serde = { version = "1.0", features = ["derive"] }
//! serde_json = "1.0"
//! ```
//!
//! ```rust,ignore
//! use diaryx_plugin_sdk::prelude::*;
//! use extism_pdk::*;
//!
//! #[plugin_fn]
//! pub fn manifest(_input: String) -> FnResult<String> {
//!     let manifest = GuestManifest::new(
//!         "diaryx.myplugin",
//!         "My Plugin",
//!         env!("CARGO_PKG_VERSION"),
//!         "Does cool things",
//!         vec!["custom_commands".into()],
//!     )
//!     .commands(vec!["my-command".into()]);
//!     Ok(serde_json::to_string(&manifest)?)
//! }
//!
//! #[plugin_fn]
//! pub fn handle_command(input: String) -> FnResult<String> {
//!     let req: CommandRequest = serde_json::from_str(&input)?;
//!     let resp = match req.command.as_str() {
//!         "my-command" => {
//!             let content = host::fs::read_file("index.md")?;
//!             CommandResponse::ok(serde_json::json!({ "length": content.len() }))
//!         }
//!         _ => CommandResponse::err(format!("Unknown command: {}", req.command)),
//!     };
//!     Ok(serde_json::to_string(&resp)?)
//! }
//!
//! #[plugin_fn]
//! pub fn on_event(_input: String) -> FnResult<String> {
//!     Ok(String::new())
//! }
//!
//! #[plugin_fn]
//! pub fn get_config(_input: String) -> FnResult<String> {
//!     Ok("{}".into())
//! }
//!
//! #[plugin_fn]
//! pub fn set_config(_input: String) -> FnResult<String> {
//!     Ok(String::new())
//! }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature          | What it enables                              |
//! |------------------|----------------------------------------------|
//! | `core` (default) | File I/O, storage, logging, timestamps       |
//! | `http`           | HTTP requests via the host                   |
//! | `secrets`        | Plugin-scoped secret storage via the host    |
//! | `ws`             | WebSocket bridge                             |
//! | `events`         | Event emission to the host                   |
//! | `plugins`        | Inter-plugin command execution               |
//! | `context`        | Runtime context queries                      |
//! | `wasi`           | WASI module execution                        |
//! | `files`          | User-provided file requests                  |
//! | `full`           | All of the above                             |

pub mod config;
#[cfg(feature = "getrandom-shim")]
pub mod getrandom_shim;
pub mod host;
pub mod protocol;
pub mod state;

/// Convenience re-exports for the most commonly used types.
pub mod prelude {
    pub use crate::host;
    pub use crate::protocol::{
        CURRENT_PROTOCOL_VERSION, CommandRequest, CommandResponse, GuestEvent, GuestManifest,
        GuestRequestedPermissions,
    };
    pub use crate::state::PluginState;
}
