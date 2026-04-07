//! Unified async backend for WASM with native OPFS/IndexedDB storage.
//!
//! This module provides a single entry point for all workspace operations,
//! working directly with native storage backends (no InMemoryFileSystem).
//!
//! ## API: `execute()` / `executeJs()`
//!
//! All operations go through the unified command API:
//!
//! ```javascript
//! import { DiaryxBackend } from './wasm/diaryx_wasm.js';
//!
//! const backend = await DiaryxBackend.createOpfs();
//!
//! // Use execute() with Command objects
//! const response = await backend.execute(JSON.stringify({
//!   type: 'GetEntry',
//!   params: { path: 'workspace/journal/2024-01-08.md' }
//! }));
//!
//! // Or executeJs() with JavaScript objects directly
//! const response = await backend.executeJs({
//!   type: 'GetWorkspaceTree',
//!   params: { path: 'workspace/index.md' }
//! });
//! ```
//!
//! ## Special Methods
//!
//! A few methods are kept outside the command API for specific reasons:
//! - `readBinary` / `writeBinary`: Efficient Uint8Array handling without base64 overhead

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use diaryx_core::diaryx::Diaryx;
use diaryx_core::fs::{
    AsyncFileSystem, CallbackRegistry, EventEmittingFs, FileSystemEvent, InMemoryFileSystem,
    SyncToAsyncFs,
};
use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

#[cfg(feature = "browser")]
use crate::fsa_fs::FsaFileSystem;
#[cfg(feature = "browser")]
use crate::indexeddb_fs::IndexedDbFileSystem;
use crate::js_async_fs::JsAsyncFileSystem;
#[cfg(feature = "browser")]
use crate::opfs_fs::OpfsFileSystem;

// ============================================================================
// Storage Backend Enum
// ============================================================================

/// Internal enum to hold either storage backend.
pub(crate) enum StorageBackend {
    #[cfg(feature = "browser")]
    Opfs(OpfsFileSystem),
    #[cfg(feature = "browser")]
    IndexedDb(IndexedDbFileSystem),
    /// File System Access API - user-selected directory on their real filesystem
    #[cfg(feature = "browser")]
    Fsa(FsaFileSystem),
    /// In-memory filesystem - used for guest mode in share sessions (web only)
    InMemory(SyncToAsyncFs<InMemoryFileSystem>),
    /// JavaScript-backed filesystem - used for Node.js/Obsidian/Electron integration
    JsAsync(JsAsyncFileSystem),
}

impl Clone for StorageBackend {
    fn clone(&self) -> Self {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => StorageBackend::Opfs(fs.clone()),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => StorageBackend::IndexedDb(fs.clone()),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => StorageBackend::Fsa(fs.clone()),
            StorageBackend::InMemory(fs) => StorageBackend::InMemory(fs.clone()),
            StorageBackend::JsAsync(fs) => StorageBackend::JsAsync(fs.clone()),
        }
    }
}

/// Generates a delegating `AsyncFileSystem` method for `StorageBackend`.
/// Each invocation expands to a `fn` that matches on the enum and forwards to the inner impl.
macro_rules! delegate_fs {
    // method(arg1: Type1, arg2: Type2, ...) -> ReturnType
    ($method:ident($($arg:ident : $ty:ty),*) -> $ret:ty) => {
        fn $method<'a>(&'a self, $($arg: &'a $ty),*) -> diaryx_core::fs::BoxFuture<'a, $ret> {
            match self {
                #[cfg(feature = "browser")]
                StorageBackend::Opfs(fs) => fs.$method($($arg),*),
                #[cfg(feature = "browser")]
                StorageBackend::IndexedDb(fs) => fs.$method($($arg),*),
                #[cfg(feature = "browser")]
                StorageBackend::Fsa(fs) => fs.$method($($arg),*),
                StorageBackend::InMemory(fs) => fs.$method($($arg),*),
                StorageBackend::JsAsync(fs) => fs.$method($($arg),*),
            }
        }
    };
}

impl AsyncFileSystem for StorageBackend {
    delegate_fs!(read_to_string(path: Path) -> IoResult<String>);
    delegate_fs!(write_file(path: Path, content: str) -> IoResult<()>);
    delegate_fs!(create_new(path: Path, content: str) -> IoResult<()>);
    delegate_fs!(delete_file(path: Path) -> IoResult<()>);
    delegate_fs!(list_md_files(dir: Path) -> IoResult<Vec<PathBuf>>);
    delegate_fs!(exists(path: Path) -> bool);
    delegate_fs!(create_dir_all(path: Path) -> IoResult<()>);
    delegate_fs!(is_dir(path: Path) -> bool);
    delegate_fs!(move_file(from: Path, to: Path) -> IoResult<()>);
    delegate_fs!(read_binary(path: Path) -> IoResult<Vec<u8>>);
    delegate_fs!(write_binary(path: Path, content: [u8]) -> IoResult<()>);
    delegate_fs!(list_files(dir: Path) -> IoResult<Vec<PathBuf>>);
    delegate_fs!(get_modified_time(path: Path) -> Option<i64>);
}

// ============================================================================
// Filesystem Type Alias
// ============================================================================

/// The decorated filesystem stack.
///
/// `EventEmittingFs<StorageBackend>` — pure event-emitting filesystem.
/// Sync is handled externally by the Extism sync plugin loaded at runtime.
type DecoratedFs = EventEmittingFs<StorageBackend>;

// ============================================================================
// WASM-specific Callback Registry
// ============================================================================

/// WASM-specific callback registry for filesystem events.
///
/// Unlike the thread-safe `CallbackRegistry` in diaryx_core, this version
/// stores JS functions directly using `Rc<RefCell>` since WASM is single-threaded.
pub(crate) struct WasmCallbackRegistry {
    callbacks: RefCell<HashMap<u64, js_sys::Function>>,
    next_id: AtomicU64,
}

impl WasmCallbackRegistry {
    pub(crate) fn new() -> Self {
        Self {
            callbacks: RefCell::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn subscribe(&self, callback: js_sys::Function) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.callbacks.borrow_mut().insert(id, callback);
        id
    }

    pub(crate) fn unsubscribe(&self, id: u64) -> bool {
        self.callbacks.borrow_mut().remove(&id).is_some()
    }

    pub(crate) fn emit(&self, event: &FileSystemEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let callbacks = self.callbacks.borrow();
            for callback in callbacks.values() {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&json));
            }
        }
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.callbacks.borrow().len()
    }
}

// ============================================================================
// Thread-local Bridge for Event Forwarding
// ============================================================================

// Thread-local storage for the WASM event registry.
// This allows the Rust CallbackRegistry to forward events to JS subscribers.
// Safe because WASM is single-threaded.
thread_local! {
    static WASM_EVENT_REGISTRY: RefCell<Option<Rc<WasmCallbackRegistry>>> = RefCell::new(None);
}

/// Create a bridge callback that forwards events from Rust's CallbackRegistry
/// to the WASM-specific WasmCallbackRegistry (which holds JS functions).
fn create_event_bridge() -> Arc<dyn Fn(&FileSystemEvent) + Send + Sync> {
    Arc::new(|event: &FileSystemEvent| {
        if matches!(event, FileSystemEvent::SendSyncMessage { .. }) {
            log::trace!("[EventBridge] Forwarding SendSyncMessage event to WASM_EVENT_REGISTRY");
        }
        WASM_EVENT_REGISTRY.with(|reg| {
            if let Some(registry) = reg.borrow().as_ref() {
                registry.emit(event);
            } else {
                log::warn!("[EventBridge] WASM_EVENT_REGISTRY is None!");
            }
        });
    })
}

// ============================================================================
// DiaryxBackend Class
// ============================================================================

/// Unified async backend with native storage.
///
/// This is the main entry point for all workspace operations in WASM.
/// It wraps either OPFS or IndexedDB storage and provides a complete
/// async API for workspace, entry, search, and validation operations.
///
/// ## Usage
///
/// All operations go through `execute()` or `executeJs()`:
///
/// ```javascript
/// const backend = await DiaryxBackend.createOpfs();
/// const response = await backend.executeJs({
///   type: 'GetEntry',
///   params: { path: 'workspace/notes.md' }
/// });
/// ```
#[wasm_bindgen]
pub struct DiaryxBackend {
    /// Filesystem stack (see `DecoratedFs` type alias).
    fs: Rc<DecoratedFs>,
    /// WASM-specific event callback registry for JS subscribers.
    wasm_event_registry: Rc<WasmCallbackRegistry>,
    /// Rust event registry that bridges to WASM registry.
    #[allow(dead_code)]
    rust_event_registry: Arc<CallbackRegistry>,
    /// Shared Diaryx instance for command execution.
    /// Created once during backend initialization with callbacks pre-configured.
    diaryx: Diaryx<DecoratedFs>,
}

impl DiaryxBackend {
    /// Internal helper: build a DiaryxBackend from a StorageBackend.
    ///
    /// Creates a lightweight event-emitting filesystem stack.
    /// Sync and publish are handled by Extism plugins loaded at runtime.
    fn build_from_storage(
        storage_backend: StorageBackend,
        _use_sqlite: bool,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        rust_event_registry.subscribe(create_event_bridge());

        // Build decorator stack: EventEmittingFs<StorageBackend> (no CrdtFs)
        let event_fs =
            EventEmittingFs::with_registry(storage_backend, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        // Note: Plugins (Publish, Sync) are loaded at runtime via the Extism browser plugin system.
        let diaryx = {
            let d = Diaryx::new((*fs).clone());
            d.set_workspace_root(PathBuf::from(""));
            d
        };

        Ok(DiaryxBackend {
            fs,
            wasm_event_registry,
            rust_event_registry,
            diaryx,
        })
    }
}

#[wasm_bindgen]
impl DiaryxBackend {
    // ========================================================================
    // Factory Methods
    // ========================================================================

    /// Create a new DiaryxBackend with OPFS storage.
    #[cfg(feature = "browser")]
    #[wasm_bindgen(js_name = "createOpfs")]
    pub async fn create_opfs(
        root_name: Option<String>,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let name = root_name.unwrap_or_else(|| "My Journal".to_string());
        let opfs = OpfsFileSystem::create_with_name(&name).await?;
        Self::build_from_storage(StorageBackend::Opfs(opfs), true)
    }

    /// Create a new DiaryxBackend with IndexedDB storage.
    #[cfg(feature = "browser")]
    #[wasm_bindgen(js_name = "createIndexedDb")]
    pub async fn create_indexed_db(
        db_name: Option<String>,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let idb = IndexedDbFileSystem::create_with_name(db_name).await?;
        Self::build_from_storage(StorageBackend::IndexedDb(idb), true)
    }

    /// Create backend with specific storage type.
    #[wasm_bindgen(js_name = "create")]
    pub async fn create(storage_type: &str) -> std::result::Result<DiaryxBackend, JsValue> {
        match storage_type.to_lowercase().as_str() {
            #[cfg(feature = "browser")]
            "opfs" => Self::create_opfs(None).await,
            #[cfg(feature = "browser")]
            "indexeddb" | "indexed_db" => Self::create_indexed_db(None).await,
            "memory" | "inmemory" | "in_memory" => Self::create_in_memory(),
            _ => Err(JsValue::from_str(&format!(
                "Unknown storage type: {}",
                storage_type
            ))),
        }
    }

    /// Create a new DiaryxBackend backed by JavaScript filesystem callbacks.
    #[wasm_bindgen(js_name = "createFromJsFileSystem")]
    pub fn create_from_js_file_system(
        callbacks: JsValue,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let js_fs = JsAsyncFileSystem::new(callbacks);
        Self::build_from_storage(StorageBackend::JsAsync(js_fs), false)
    }

    /// Create a new DiaryxBackend with in-memory storage.
    #[wasm_bindgen(js_name = "createInMemory")]
    pub fn create_in_memory() -> std::result::Result<DiaryxBackend, JsValue> {
        let mem_fs = InMemoryFileSystem::new();
        let async_fs = SyncToAsyncFs::new(mem_fs);
        Self::build_from_storage(StorageBackend::InMemory(async_fs), false)
    }

    /// Create a new DiaryxBackend from a user-selected directory handle.
    #[cfg(feature = "browser")]
    #[wasm_bindgen(js_name = "createFromDirectoryHandle")]
    pub fn create_from_directory_handle(
        handle: web_sys::FileSystemDirectoryHandle,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        let fsa = FsaFileSystem::from_handle(handle);
        Self::build_from_storage(StorageBackend::Fsa(fsa), true)
    }

    // ========================================================================
    // CrdtFs Control (no-op — sync handled by Extism plugin)
    // ========================================================================

    /// No-op — CrdtFs is not used; sync handled by Extism plugin.
    #[wasm_bindgen(js_name = "setCrdtEnabled")]
    pub fn set_crdt_enabled(&self, _enabled: bool) {}

    /// Always returns false — CrdtFs is not used; sync handled by Extism plugin.
    #[wasm_bindgen(js_name = "isCrdtEnabled")]
    pub fn is_crdt_enabled(&self) -> bool {
        false
    }

    // ========================================================================
    // Unified Command API
    // ========================================================================

    /// Execute a command and return the response as JSON string.
    ///
    /// This is the primary unified API for all operations.
    ///
    /// ## Example
    /// ```javascript
    /// const command = { type: 'GetEntry', params: { path: 'workspace/notes.md' } };
    /// const responseJson = await backend.execute(JSON.stringify(command));
    /// const response = JSON.parse(responseJson);
    /// ```
    #[wasm_bindgen]
    pub async fn execute(&self, command_json: &str) -> std::result::Result<String, JsValue> {
        use diaryx_core::Command;

        // Parse the command from JSON
        let cmd: Command = serde_json::from_str(command_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid command JSON: {}", e)))?;

        // Execute the command using the shared Diaryx instance
        // (callbacks were configured once during backend creation)
        let result = self
            .diaryx
            .execute(cmd)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Serialize the response to JSON
        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize response: {}", e)))
    }

    // ========================================================================
    // Binary Operations (kept for efficiency - no base64 overhead)
    // ========================================================================

    /// Read binary file.
    ///
    /// Returns data as Uint8Array for efficient handling without base64 encoding.
    #[wasm_bindgen(js_name = "readBinary")]
    pub fn read_binary(&self, path: String) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let data = fs
                .read_binary(&PathBuf::from(&path))
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(js_sys::Uint8Array::from(data.as_slice()).into())
        })
    }

    /// Write binary file.
    ///
    /// Accepts Uint8Array for efficient handling without base64 encoding.
    #[wasm_bindgen(js_name = "writeBinary")]
    pub fn write_binary(&self, path: String, data: js_sys::Uint8Array) -> Promise {
        let fs = self.fs.clone();
        let data_vec = data.to_vec();

        future_to_promise(async move {
            fs.write_binary(&PathBuf::from(&path), &data_vec)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    // ========================================================================
    // Event Subscription API
    // ========================================================================

    /// Subscribe to filesystem events.
    ///
    /// The callback will be invoked with a JSON-serialized FileSystemEvent
    /// whenever filesystem operations occur (create, delete, rename, move, etc.).
    ///
    /// Returns a subscription ID that can be used to unsubscribe later.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// const id = backend.onFileSystemEvent((eventJson) => {
    ///     const event = JSON.parse(eventJson);
    ///     console.log('File event:', event.type, event.path);
    /// });
    ///
    /// // Later, to unsubscribe:
    /// backend.offFileSystemEvent(id);
    /// ```
    #[wasm_bindgen(js_name = "onFileSystemEvent")]
    pub fn on_filesystem_event(&self, callback: js_sys::Function) -> u64 {
        self.wasm_event_registry.subscribe(callback)
    }

    /// Unsubscribe from filesystem events.
    ///
    /// Returns `true` if the subscription was found and removed.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// const id = backend.onFileSystemEvent(handler);
    /// // ... later ...
    /// const removed = backend.offFileSystemEvent(id);
    /// console.log('Subscription removed:', removed);
    /// ```
    #[wasm_bindgen(js_name = "offFileSystemEvent")]
    pub fn off_filesystem_event(&self, id: u64) -> bool {
        self.wasm_event_registry.unsubscribe(id)
    }

    /// Emit a filesystem event.
    ///
    /// This is primarily used internally but can be called from JavaScript
    /// to manually trigger events (e.g., for testing or manual sync scenarios).
    ///
    /// The event should be a JSON string matching the FileSystemEvent format.
    ///
    /// ## Example
    ///
    /// ```javascript
    /// backend.emitFileSystemEvent(JSON.stringify({
    ///     type: 'FileCreated',
    ///     path: 'workspace/notes.md',
    ///     frontmatter: { title: 'Notes' }
    /// }));
    /// ```
    #[wasm_bindgen(js_name = "emitFileSystemEvent")]
    pub fn emit_filesystem_event(&self, event_json: &str) -> std::result::Result<(), JsValue> {
        let event: FileSystemEvent = serde_json::from_str(event_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid event JSON: {}", e)))?;
        self.wasm_event_registry.emit(&event);
        Ok(())
    }

    /// Get the number of active event subscriptions.
    #[wasm_bindgen(js_name = "eventSubscriberCount")]
    pub fn event_subscriber_count(&self) -> usize {
        self.wasm_event_registry.subscriber_count()
    }

    /// Check if this backend has native sync support.
    /// Always false — sync is handled by the Extism sync plugin loaded at runtime.
    #[wasm_bindgen(js_name = "hasNativeSync")]
    pub fn has_native_sync(&self) -> bool {
        false
    }
}
