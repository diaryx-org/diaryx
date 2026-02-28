//! Extism-based third-party plugin runtime for Diaryx.
//!
//! Loads WebAssembly plugin modules via the [Extism](https://extism.org/) runtime
//! and adapts them to the diaryx_core [`Plugin`], [`WorkspacePlugin`], and [`FilePlugin`]
//! traits. Guest plugins communicate with the host through a JSON protocol defined
//! in [`protocol`].
//!
//! # Usage
//!
//! ```ignore
//! use diaryx_extism::{HostContext, load_plugins_from_dir};
//! use std::sync::Arc;
//!
//! let host_ctx = Arc::new(HostContext { fs: my_async_fs.clone() });
//! let plugins = load_plugins_from_dir(&plugins_dir, host_ctx)?;
//! for plugin in plugins {
//!     let arc = Arc::new(plugin);
//!     registry.register_workspace_plugin(arc.clone());
//!     registry.register_file_plugin(arc);
//! }
//! ```

pub mod adapter;
pub mod binary_protocol;
pub mod host_fns;
pub mod loader;
pub mod protocol;

pub use adapter::ExtismPluginAdapter;
pub use host_fns::{EventEmitter, HostContext, NoopEventEmitter, NoopStorage, PluginStorage};
pub use loader::{ExtismLoadError, load_plugin_from_wasm, load_plugins_from_dir};
