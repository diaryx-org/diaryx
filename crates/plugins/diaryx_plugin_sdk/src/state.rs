//! State management helpers for WASM plugins.
//!
//! Provides the common `OnceLock<Mutex<T>>` pattern used across plugins
//! for managing global plugin state in a single-threaded WASM environment.

use std::sync::{Mutex, OnceLock};

/// A thread-safe global state container for plugin state.
///
/// # Example
///
/// ```rust,ignore
/// use diaryx_plugin_sdk::state::PluginState;
///
/// #[derive(Debug, Clone, Default)]
/// struct MyState {
///     workspace_root: Option<String>,
///     count: u32,
/// }
///
/// static STATE: PluginState<MyState> = PluginState::new();
///
/// // Read state
/// let current = STATE.get();
///
/// // Update state
/// STATE.update(|s| s.count += 1);
/// ```
pub struct PluginState<T> {
    inner: OnceLock<Mutex<T>>,
}

impl<T: Default + Clone> PluginState<T> {
    /// Create a new uninitialized state container.
    pub const fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    /// Get a clone of the current state, initializing with `Default` if needed.
    pub fn get(&self) -> T {
        self.inner
            .get_or_init(|| Mutex::new(T::default()))
            .lock()
            .expect("PluginState lock poisoned")
            .clone()
    }

    /// Update the state with a closure, initializing with `Default` if needed.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let mut guard = self
            .inner
            .get_or_init(|| Mutex::new(T::default()))
            .lock()
            .expect("PluginState lock poisoned");
        f(&mut guard);
    }

    /// Replace the entire state.
    pub fn set(&self, value: T) {
        self.update(|s| *s = value);
    }

    /// Get a clone of the state, returning `None` if not yet initialized.
    pub fn try_get(&self) -> Option<T> {
        self.inner
            .get()
            .map(|m| m.lock().expect("PluginState lock poisoned").clone())
    }
}
