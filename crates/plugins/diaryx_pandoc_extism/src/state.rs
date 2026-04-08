//! Thread-local state management for the pandoc Extism guest.

use std::cell::RefCell;
use std::path::PathBuf;

/// State held by the pandoc plugin guest for the lifetime of the WASM instance.
pub struct PluginState {
    pub workspace_root: Option<PathBuf>,
}

thread_local! {
    static STATE: RefCell<Option<PluginState>> = const { RefCell::new(None) };
}

/// Initialize the plugin state.
pub fn init_state() -> Result<(), String> {
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        if borrow.is_some() {
            return Ok(());
        }
        *borrow = Some(PluginState {
            workspace_root: None,
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

/// Access plugin state mutably.
pub fn with_state_mut<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut PluginState) -> R,
{
    STATE.with(|s| {
        let mut borrow = s.borrow_mut();
        let state = borrow
            .as_mut()
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
