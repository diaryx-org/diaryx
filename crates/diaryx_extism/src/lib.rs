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

/// Returns a `wasmtime::Config` configured for the Pulley interpreter plus
/// iOS-safe linear-memory reservations on mobile Apple builds.
///
/// App Store / TestFlight iOS builds reject Wasmtime's default 4 GiB virtual
/// address reservation for 32-bit Wasm linear memories. Lowering the
/// reservation trades a few bounds-check optimizations for successful plugin
/// instantiation inside the sandbox.
#[cfg(target_os = "ios")]
pub(crate) fn platform_wasmtime_config() -> Option<wasmtime::Config> {
    let mut config = wasmtime::Config::new();
    if let Err(e) = config.target("pulley64") {
        log::warn!("Failed to set Pulley target for wasmtime: {e}");
        return None;
    }
    config.memory_reservation(10 * (1 << 20));
    config.memory_guard_size(0x1_0000);
    config.memory_reservation_for_growth(1 << 20);
    Some(config)
}

/// Returns a `wasmtime::Config` configured for the Pulley interpreter on
/// platforms that don't support JIT (for example App Store macOS builds),
/// or `None` to use the default Cranelift JIT elsewhere.
///
/// App Store / TestFlight macOS builds run inside the same hardened sandbox
/// that restricts iOS. Wasmtime's default 4 GiB virtual address reservation
/// for 32-bit Wasm linear memories is rejected by the sandbox, causing
/// out-of-bounds memory traps when the guest reads input. Apply the same
/// reduced memory limits as iOS.
#[cfg(all(not(target_os = "ios"), feature = "pulley"))]
pub(crate) fn platform_wasmtime_config() -> Option<wasmtime::Config> {
    let mut config = wasmtime::Config::new();
    if let Err(e) = config.target("pulley64") {
        log::warn!("Failed to set Pulley target for wasmtime: {e}");
        return None;
    }
    config.memory_reservation(10 * (1 << 20));
    config.memory_guard_size(0x1_0000);
    config.memory_reservation_for_growth(1 << 20);
    Some(config)
}

#[cfg(not(any(feature = "pulley", target_os = "ios")))]
pub(crate) fn platform_wasmtime_config() -> Option<wasmtime::Config> {
    None
}

pub mod adapter;
pub mod binary_protocol;
pub mod host_fns;
pub mod loader;
pub mod permission_checker;
pub mod plugin_fs;
pub mod protocol;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(feature = "wasi-runner")]
pub mod wasi_runner;
#[cfg(feature = "ws-transport")]
pub mod ws_transport;

pub use adapter::ExtismPluginAdapter;
pub use host_fns::{
    BatchGetEntry, BatchGetResult, DEFAULT_STORAGE_QUOTA_BYTES, EventEmitter,
    FilePluginSecretStore, FilePluginStorage, FileProvider, HostContext, MapFileProvider,
    NamespaceEntry, NamespaceObjectMeta, NamespaceProvider, NoopEventEmitter, NoopFileProvider,
    NoopNamespaceProvider, NoopPluginCommandBridge, NoopRuntimeContextProvider, NoopSecretStore,
    NoopStorage, NoopWebSocketBridge, PermissionChecker, PluginCommandBridge, PluginSecretStore,
    PluginStorage, RuntimeContextProvider, WebSocketBridge,
};
pub use loader::{
    ExtismLoadError, inspect_plugin_wasm_manifest, load_plugin_from_wasm, load_plugins_from_dir,
};
pub use permission_checker::{
    AllowAllPermissionChecker, DenyAllPermissionChecker, FrontmatterPermissionChecker,
};

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "pulley")]
    fn pulley_config_creates_plugin() {
        let config =
            super::platform_wasmtime_config().expect("pulley feature should produce config");

        let manifest = extism::Manifest::new([extism::Wasm::data(b"\x00asm\x01\x00\x00\x00")]);
        let result = extism::PluginBuilder::new(manifest)
            .with_wasmtime_config(config)
            .build();
        assert!(
            result.is_ok(),
            "Pulley plugin build failed: {:?}",
            result.err()
        );
    }
}
pub use plugin_fs::PluginFileSystem;
#[cfg(feature = "ws-transport")]
pub use ws_transport::{SyncGuestBridge, TokioWebSocketBridge};
