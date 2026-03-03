//! Thread-local state management for the publish Extism guest.
//!
//! WASM is single-threaded, so we use `thread_local!` + `RefCell` to store
//! the plugin state. All access is through `with_state()` / `with_state_mut()`.

use std::cell::RefCell;

use diaryx_publish::PublishPlugin;

use crate::host_fs::HostFs;

/// State held by the publish plugin guest for the lifetime of the WASM instance.
pub struct PluginState {
    /// The inner publish plugin (handles export commands).
    pub publish_plugin: PublishPlugin<HostFs>,
}

thread_local! {
    static STATE: RefCell<Option<PluginState>> = const { RefCell::new(None) };
}

/// Initialize the plugin state.
pub fn init_state() -> Result<(), String> {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        if borrow.is_some() {
            return Ok(()); // Already initialized
        }
        let plugin = PublishPlugin::new(HostFs);
        *borrow = Some(PluginState {
            publish_plugin: plugin,
        });
        Ok(())
    })
}

/// Access plugin state immutably.
pub fn with_state<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&PluginState) -> R,
{
    STATE.with(|s| {
        let borrow = s.borrow();
        let state = borrow
            .as_ref()
            .ok_or_else(|| "Plugin state not initialized".to_string())?;
        Ok(f(state))
    })
}

/// Shut down the plugin state.
pub fn shutdown_state() -> Result<(), String> {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        *borrow = None;
        Ok(())
    })
}
