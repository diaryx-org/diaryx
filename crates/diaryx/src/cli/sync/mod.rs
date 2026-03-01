//! Sync command handlers for the CLI.
//!
//! Individual handler functions are called by the plugin dispatch system
//! via native handlers registered in `plugin_dispatch.rs`.

pub mod auth;
pub mod client;
pub(crate) mod progress;
pub mod status;
pub mod ws_bridge;
