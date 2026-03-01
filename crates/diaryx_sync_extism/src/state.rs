//! Thread-local plugin state for the Extism guest.
//!
//! Since WASM is single-threaded, we use `RefCell`-based global state
//! to hold the SyncPlugin, SyncSession, and related objects.

use std::cell::RefCell;
use std::sync::Arc;

use diaryx_sync::{
    CrdtStorage, MemoryStorage, SyncEvent, SyncPlugin, SyncSession, SyncSessionConfig,
};

use crate::host_bridge;
use crate::host_fs::HostFs;

/// All mutable state owned by the guest plugin.
pub struct PluginState {
    /// The sync plugin wrapping CRDT state.
    pub sync_plugin: SyncPlugin<HostFs>,
    /// The sync session protocol handler (created on connect).
    pub session: Option<SyncSession<HostFs>>,
    /// Workspace ID (set during init).
    pub workspace_id: Option<String>,
    /// Whether to write changes to disk.
    pub write_to_disk: bool,
}

thread_local! {
    static STATE: RefCell<Option<PluginState>> = const { RefCell::new(None) };
}

/// Initialize the plugin state with fresh CRDT state.
pub fn init_state(workspace_id: Option<String>) -> Result<(), &'static str> {
    let fs = HostFs;
    let storage: Arc<dyn CrdtStorage> = Arc::new(MemoryStorage::new());

    // Try to load persisted workspace CRDT state
    if let Ok(Some(data)) = host_bridge::storage_get("workspace_crdt") {
        if let Err(e) = storage.save_doc("workspace", &data) {
            host_bridge::log_message("warn", &format!("Failed to restore workspace CRDT: {e}"));
        }
    }

    let sync_plugin = SyncPlugin::new(fs, storage);

    STATE.with(|s| {
        let mut borrow = s
            .try_borrow_mut()
            .map_err(|_| "Plugin state already mutably borrowed")?;
        *borrow = Some(PluginState {
            sync_plugin,
            session: None,
            workspace_id,
            write_to_disk: true,
        });
        Ok(())
    })
}

/// Access plugin state immutably.
pub fn with_state<F, R>(f: F) -> Result<R, &'static str>
where
    F: FnOnce(&PluginState) -> R,
{
    STATE.with(|s| {
        let borrow = s
            .try_borrow()
            .map_err(|_| "Plugin state already mutably borrowed")?;
        let state = borrow.as_ref().ok_or("Plugin state not initialized")?;
        Ok(f(state))
    })
}

/// Mutably access the plugin state.
pub fn with_state_mut<F, R>(f: F) -> Result<R, &'static str>
where
    F: FnOnce(&mut PluginState) -> R,
{
    STATE.with(|s| {
        let mut borrow = s
            .try_borrow_mut()
            .map_err(|_| "Plugin state already mutably borrowed")?;
        let state = borrow.as_mut().ok_or("Plugin state not initialized")?;
        Ok(f(state))
    })
}

/// Try to mutably access plugin state without panicking on borrow conflicts.
///
/// Returns an error when state is uninitialized or currently mutably borrowed.
pub fn try_with_state_mut<F, R>(f: F) -> Result<R, &'static str>
where
    F: FnOnce(&mut PluginState) -> R,
{
    with_state_mut(f)
}

/// Create a SyncSession from current state.
pub fn create_session(workspace_id: &str, write_to_disk: bool) -> Result<(), &'static str> {
    try_with_state_mut(|state| {
        let config = SyncSessionConfig {
            workspace_id: workspace_id.to_string(),
            write_to_disk,
        };
        let session = SyncSession::new(config, state.sync_plugin.sync_manager());
        state.session = Some(session);
        state.workspace_id = Some(workspace_id.to_string());
        state.write_to_disk = write_to_disk;
    })
}

/// Persist the current CRDT state via host storage.
pub fn persist_state() -> Result<(), &'static str> {
    STATE.with(|s| {
        let borrow = s
            .try_borrow()
            .map_err(|_| "Plugin state already mutably borrowed")?;
        let state = borrow.as_ref().ok_or("Plugin state not initialized")?;

        // Save workspace CRDT
        let ws_crdt = state.sync_plugin.workspace_crdt();
        let ws_state = ws_crdt.encode_state_as_update();
        if let Err(e) = host_bridge::storage_set("workspace_crdt", &ws_state) {
            host_bridge::log_message("warn", &format!("Failed to persist workspace CRDT: {e}"));
        }

        // Save all body docs
        let body_docs = state.sync_plugin.body_docs();
        if let Err(e) = body_docs.save_all() {
            host_bridge::log_message("warn", &format!("Failed to save body docs: {e}"));
        }

        // Persist each body doc's full state via host storage
        for doc_name in body_docs.loaded_docs() {
            if let Some(doc) = body_docs.get(&doc_name) {
                let doc_state = doc.encode_state_as_update();
                let key = format!("body:{doc_name}");
                if let Err(e) = host_bridge::storage_set(&key, &doc_state) {
                    host_bridge::log_message(
                        "warn",
                        &format!("Failed to persist body doc {doc_name}: {e}"),
                    );
                }
            }
        }
        Ok(())
    })
}

/// Emit a sync event to the host.
pub fn emit_sync_event(event: &SyncEvent) {
    if let Ok(json) = serde_json::to_string(event) {
        let _ = host_bridge::emit_event(&json);
    }
}

/// Shut down the plugin state.
pub fn shutdown_state() -> Result<(), &'static str> {
    if let Err(e) = persist_state() {
        host_bridge::log_message(
            "warn",
            &format!("Failed to persist plugin state on shutdown: {e}"),
        );
    }
    STATE.with(|s| {
        let mut borrow = s
            .try_borrow_mut()
            .map_err(|_| "Plugin state already mutably borrowed")?;
        *borrow = None;
        Ok(())
    })
}

/// Check if state is initialized.
pub fn is_initialized() -> bool {
    STATE.with(|s| match s.try_borrow() {
        Ok(borrow) => borrow.is_some(),
        Err(_) => true,
    })
}
