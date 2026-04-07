//! Thread-local plugin state for the LWW file sync engine.
//!
//! Since WASM is single-threaded, we use `RefCell`-based global state
//! to hold the sync manifest and workspace configuration.

use std::cell::RefCell;

use crate::sync_manifest::SyncManifest;

thread_local! {
    static MANIFEST: RefCell<Option<SyncManifest>> = const { RefCell::new(None) };
    static WORKSPACE_ROOT: RefCell<Option<String>> = const { RefCell::new(None) };
    static NAMESPACE_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Initialize the plugin state, loading manifest from storage if available.
pub fn init_state(namespace_id: Option<String>, workspace_root: Option<String>) {
    let manifest = namespace_id
        .as_deref()
        .map(SyncManifest::load)
        .unwrap_or_else(|| SyncManifest::new(String::new()));

    MANIFEST.with(|m| *m.borrow_mut() = Some(manifest));
    WORKSPACE_ROOT.with(|w| *w.borrow_mut() = workspace_root);
    NAMESPACE_ID.with(|n| *n.borrow_mut() = namespace_id);
}

/// Save manifest and clean up on shutdown.
pub fn shutdown_state() {
    MANIFEST.with(|m| {
        if let Some(manifest) = m.borrow().as_ref() {
            manifest.save();
        }
        *m.borrow_mut() = None;
    });
    WORKSPACE_ROOT.with(|w| *w.borrow_mut() = None);
    NAMESPACE_ID.with(|n| *n.borrow_mut() = None);
}

/// Access the sync manifest.
pub fn with_manifest<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&SyncManifest) -> R,
{
    MANIFEST.with(|m| m.borrow().as_ref().map(f))
}

/// Mutably access the sync manifest.
pub fn with_manifest_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut SyncManifest) -> R,
{
    MANIFEST.with(|m| m.borrow_mut().as_mut().map(f))
}

/// Get the current workspace root.
pub fn workspace_root() -> Option<String> {
    WORKSPACE_ROOT.with(|w| w.borrow().clone())
}

/// Get the current namespace ID.
pub fn namespace_id() -> Option<String> {
    NAMESPACE_ID.with(|n| n.borrow().clone())
}

/// Update the namespace ID and reload the manifest.
pub fn set_namespace_id(ns_id: Option<String>) -> Result<(), String> {
    let manifest = ns_id
        .as_deref()
        .map(SyncManifest::load)
        .unwrap_or_else(|| SyncManifest::new(String::new()));

    MANIFEST.with(|m| {
        let mut slot = m
            .try_borrow_mut()
            .map_err(|_| "Sync state is busy updating the manifest. Try again.".to_string())?;
        *slot = Some(manifest);
        Ok::<(), String>(())
    })?;
    NAMESPACE_ID.with(|n| {
        let mut slot = n.try_borrow_mut().map_err(|_| {
            "Sync state is busy updating the workspace link. Try again.".to_string()
        })?;
        *slot = ns_id;
        Ok::<(), String>(())
    })?;
    Ok(())
}

/// Check if state is initialized.
pub fn is_initialized() -> bool {
    MANIFEST.with(|m| m.borrow().is_some())
}
