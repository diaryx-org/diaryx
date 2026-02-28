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
//! - `getConfig` / `saveConfig`: WASM-specific config stored in root frontmatter
//! - `readBinary` / `writeBinary`: Efficient Uint8Array handling without base64 overhead

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Result as IoResult;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use diaryx_core::diaryx::Diaryx;
use diaryx_core::frontmatter;
use diaryx_core::fs::{
    AsyncFileSystem, CallbackRegistry, EventEmittingFs, FileSystemEvent, InMemoryFileSystem,
    SyncToAsyncFs,
};
use diaryx_core::workspace::Workspace;
#[cfg(feature = "sync")]
use diaryx_sync::{
    BodyDocManager, CrdtFs, CrdtStorage, MemoryStorage, RustSyncManager, SyncMessage, SyncPlugin,
    SyncSessionConfig, WorkspaceCrdt,
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
#[cfg(feature = "sync")]
use crate::wasm_sqlite_storage::WasmSqliteStorage;

// ============================================================================
// Storage Backend Enum
// ============================================================================

/// Internal enum to hold either storage backend.
/// Exposed for use by wasm_sync_client module.
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

// Implement AsyncFileSystem by delegating to inner type
impl AsyncFileSystem for StorageBackend {
    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<String>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.read_to_string(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.read_to_string(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.read_to_string(path),
            StorageBackend::InMemory(fs) => fs.read_to_string(path),
            StorageBackend::JsAsync(fs) => fs.read_to_string(path),
        }
    }

    fn write_file<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.write_file(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.write_file(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.write_file(path, content),
            StorageBackend::InMemory(fs) => fs.write_file(path, content),
            StorageBackend::JsAsync(fs) => fs.write_file(path, content),
        }
    }

    fn create_new<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.create_new(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.create_new(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.create_new(path, content),
            StorageBackend::InMemory(fs) => fs.create_new(path, content),
            StorageBackend::JsAsync(fs) => fs.create_new(path, content),
        }
    }

    fn delete_file<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.delete_file(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.delete_file(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.delete_file(path),
            StorageBackend::InMemory(fs) => fs.delete_file(path),
            StorageBackend::JsAsync(fs) => fs.delete_file(path),
        }
    }

    fn list_md_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.list_md_files(dir),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.list_md_files(dir),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.list_md_files(dir),
            StorageBackend::InMemory(fs) => fs.list_md_files(dir),
            StorageBackend::JsAsync(fs) => fs.list_md_files(dir),
        }
    }

    fn exists<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.exists(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.exists(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.exists(path),
            StorageBackend::InMemory(fs) => fs.exists(path),
            StorageBackend::JsAsync(fs) => fs.exists(path),
        }
    }

    fn create_dir_all<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.create_dir_all(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.create_dir_all(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.create_dir_all(path),
            StorageBackend::InMemory(fs) => fs.create_dir_all(path),
            StorageBackend::JsAsync(fs) => fs.create_dir_all(path),
        }
    }

    fn is_dir<'a>(&'a self, path: &'a Path) -> diaryx_core::fs::BoxFuture<'a, bool> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.is_dir(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.is_dir(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.is_dir(path),
            StorageBackend::InMemory(fs) => fs.is_dir(path),
            StorageBackend::JsAsync(fs) => fs.is_dir(path),
        }
    }

    fn move_file<'a>(
        &'a self,
        from: &'a Path,
        to: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.move_file(from, to),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.move_file(from, to),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.move_file(from, to),
            StorageBackend::InMemory(fs) => fs.move_file(from, to),
            StorageBackend::JsAsync(fs) => fs.move_file(from, to),
        }
    }

    fn read_binary<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<u8>>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.read_binary(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.read_binary(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.read_binary(path),
            StorageBackend::InMemory(fs) => fs.read_binary(path),
            StorageBackend::JsAsync(fs) => fs.read_binary(path),
        }
    }

    fn write_binary<'a>(
        &'a self,
        path: &'a Path,
        content: &'a [u8],
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<()>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.write_binary(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.write_binary(path, content),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.write_binary(path, content),
            StorageBackend::InMemory(fs) => fs.write_binary(path, content),
            StorageBackend::JsAsync(fs) => fs.write_binary(path, content),
        }
    }

    fn list_files<'a>(
        &'a self,
        dir: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, IoResult<Vec<PathBuf>>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.list_files(dir),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.list_files(dir),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.list_files(dir),
            StorageBackend::InMemory(fs) => fs.list_files(dir),
            StorageBackend::JsAsync(fs) => fs.list_files(dir),
        }
    }

    fn get_modified_time<'a>(
        &'a self,
        path: &'a Path,
    ) -> diaryx_core::fs::BoxFuture<'a, Option<i64>> {
        match self {
            #[cfg(feature = "browser")]
            StorageBackend::Opfs(fs) => fs.get_modified_time(path),
            #[cfg(feature = "browser")]
            StorageBackend::IndexedDb(fs) => fs.get_modified_time(path),
            #[cfg(feature = "browser")]
            StorageBackend::Fsa(fs) => fs.get_modified_time(path),
            StorageBackend::InMemory(fs) => fs.get_modified_time(path),
            StorageBackend::JsAsync(fs) => fs.get_modified_time(path),
        }
    }
}

// ============================================================================
// Filesystem Type Alias
// ============================================================================

/// The decorated filesystem stack.
///
/// With the `sync` feature: `EventEmittingFs<CrdtFs<StorageBackend>>` — file
/// writes automatically update CRDTs for sync.
///
/// Without the `sync` feature: `EventEmittingFs<StorageBackend>` — pure
/// event-emitting filesystem, sync handled externally by Extism plugin.
#[cfg(feature = "sync")]
type DecoratedFs = EventEmittingFs<CrdtFs<StorageBackend>>;

#[cfg(not(feature = "sync"))]
type DecoratedFs = EventEmittingFs<StorageBackend>;

// ============================================================================
// WASM-specific Callback Registry
// ============================================================================

/// WASM-specific callback registry for filesystem events.
///
/// Unlike the thread-safe `CallbackRegistry` in diaryx_core, this version
/// stores JS functions directly using `Rc<RefCell>` since WASM is single-threaded.
struct WasmCallbackRegistry {
    callbacks: RefCell<HashMap<u64, js_sys::Function>>,
    next_id: AtomicU64,
}

impl WasmCallbackRegistry {
    fn new() -> Self {
        Self {
            callbacks: RefCell::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn subscribe(&self, callback: js_sys::Function) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.callbacks.borrow_mut().insert(id, callback);
        id
    }

    fn unsubscribe(&self, id: u64) -> bool {
        self.callbacks.borrow_mut().remove(&id).is_some()
    }

    fn emit(&self, event: &FileSystemEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let callbacks = self.callbacks.borrow();
            for callback in callbacks.values() {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&json));
            }
        }
    }

    fn subscriber_count(&self) -> usize {
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

// Thread-local storage for CRDT update sync callback.
// This allows observe_updates() callbacks to emit sync messages without complex lifetimes.
// Safe because WASM is single-threaded.
#[cfg(feature = "sync")]
thread_local! {
    static CRDT_SYNC_CALLBACK: RefCell<Option<Box<dyn Fn(&[u8])>>> = RefCell::new(None);
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

/// Set up the CRDT sync callback that will be called on any CRDT update.
/// This enables automatic sync emission whenever the workspace CRDT changes.
#[cfg(feature = "sync")]
fn setup_crdt_sync_callback(wasm_registry: &Rc<WasmCallbackRegistry>) {
    let registry = Rc::clone(wasm_registry);
    CRDT_SYNC_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(Box::new(move |update: &[u8]| {
            if registry.subscriber_count() > 0 {
                log::trace!(
                    "[CRDT_SYNC_CALLBACK] Emitting {} byte update, subscribers: {}",
                    update.len(),
                    registry.subscriber_count()
                );
                let encoded = SyncMessage::Update(update.to_vec()).encode();
                let event = FileSystemEvent::send_sync_message("workspace", encoded, false);
                registry.emit(&event);
            } else {
                log::trace!(
                    "[CRDT_SYNC_CALLBACK] No subscribers, dropping {} byte update",
                    update.len()
                );
            }
        }));
    });
}

/// Create a subscription to workspace CRDT updates that emits sync messages.
/// The subscription callback accesses the thread-local CRDT_SYNC_CALLBACK.
#[cfg(feature = "sync")]
fn subscribe_to_crdt_updates(workspace_crdt: &Arc<WorkspaceCrdt>) -> yrs::Subscription {
    workspace_crdt.observe_updates(|update| {
        CRDT_SYNC_CALLBACK.with(|cb| {
            if let Some(ref callback) = *cb.borrow() {
                callback(update);
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
    /// CRDT storage for sync and history features.
    #[cfg(feature = "sync")]
    #[allow(dead_code)]
    crdt_storage: Arc<dyn CrdtStorage>,
    /// Workspace CRDT for file metadata sync.
    #[cfg(feature = "sync")]
    #[allow(dead_code)]
    workspace_crdt: Arc<WorkspaceCrdt>,
    /// Body document manager for file content sync.
    #[cfg(feature = "sync")]
    #[allow(dead_code)]
    body_doc_manager: Arc<BodyDocManager>,
    /// WASM-specific event callback registry for JS subscribers.
    wasm_event_registry: Rc<WasmCallbackRegistry>,
    /// Rust event registry that bridges to WASM registry.
    #[allow(dead_code)]
    rust_event_registry: Arc<CallbackRegistry>,
    /// Subscription to CRDT updates for automatic sync emission.
    /// Must be stored to prevent the subscription from being dropped.
    #[cfg(feature = "sync")]
    #[allow(dead_code)]
    crdt_update_subscription: Option<yrs::Subscription>,
    /// Sync manager for handling sync protocol messages.
    /// Shared across all sync operations for persistent state.
    #[cfg(feature = "sync")]
    sync_manager: Arc<RustSyncManager<DecoratedFs>>,
    /// Shared Diaryx instance for command execution.
    /// Created once during backend initialization with callbacks pre-configured.
    diaryx: Diaryx<DecoratedFs>,
}

impl DiaryxBackend {
    /// Internal helper: build a DiaryxBackend from a StorageBackend.
    ///
    /// When the `sync` feature is enabled, this creates the full CRDT stack
    /// (CrdtFs, SyncPlugin, etc.). Without `sync`, it creates a lightweight
    /// event-only filesystem stack.
    #[cfg(feature = "sync")]
    fn build_from_storage(
        storage_backend: StorageBackend,
        use_sqlite: bool,
    ) -> std::result::Result<DiaryxBackend, JsValue> {
        // Create event registries
        let wasm_event_registry = Rc::new(WasmCallbackRegistry::new());
        let rust_event_registry = Arc::new(CallbackRegistry::new());

        WASM_EVENT_REGISTRY.with(|reg| {
            *reg.borrow_mut() = Some(Rc::clone(&wasm_event_registry));
        });

        rust_event_registry.subscribe(create_event_bridge());

        // CRDT storage
        let crdt_storage: Arc<dyn CrdtStorage> = if use_sqlite {
            match WasmSqliteStorage::new() {
                Ok(storage) => {
                    log::info!("✓ CRDT storage: Using persistent SQLite storage");
                    Arc::new(storage)
                }
                Err(e) => {
                    log::error!(
                        "✗ CRDT storage: FALLBACK TO MEMORY - {:?}. This will cause data loss!",
                        e
                    );
                    Arc::new(MemoryStorage::new())
                }
            }
        } else {
            Arc::new(MemoryStorage::new())
        };

        // Create shared CRDT instances with event callbacks
        let workspace_crdt = {
            let mut crdt = if use_sqlite {
                WorkspaceCrdt::load(Arc::clone(&crdt_storage))
                    .map_err(|e| JsValue::from_str(&format!("Failed to load CRDT: {}", e)))?
            } else {
                WorkspaceCrdt::new(Arc::clone(&crdt_storage))
            };
            let registry = Arc::clone(&rust_event_registry);
            crdt.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(crdt)
        };

        let body_doc_manager = {
            let manager = BodyDocManager::new(Arc::clone(&crdt_storage));
            let registry = Arc::clone(&rust_event_registry);
            manager.set_event_callback(Arc::new(move |event| {
                registry.emit(event);
            }));
            Arc::new(manager)
        };

        // Build decorator stack: EventEmittingFs<CrdtFs<StorageBackend>>
        let crdt_fs = CrdtFs::new(
            storage_backend,
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
        );
        let event_fs = EventEmittingFs::with_registry(crdt_fs, Arc::clone(&rust_event_registry));
        let fs = Rc::new(event_fs);

        // CRDT sync callback and subscription
        setup_crdt_sync_callback(&wasm_event_registry);
        let crdt_update_subscription = subscribe_to_crdt_updates(&workspace_crdt);

        // Create SyncPlugin and extract sync_manager handle
        let sync_plugin = SyncPlugin::with_instances(
            (*fs).clone(),
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_doc_manager),
            Arc::clone(&crdt_storage),
        );
        let sync_manager = sync_plugin.sync_manager();
        sync_manager.set_event_callback(create_event_bridge());

        // Create shared Diaryx instance with SyncPlugin registered
        let diaryx = {
            let mut d = Diaryx::new((*fs).clone());
            d.plugin_registry_mut()
                .register_workspace_plugin(Arc::new(sync_plugin));
            d.set_workspace_root(PathBuf::from(""));
            d
        };

        Ok(DiaryxBackend {
            fs,
            crdt_storage,
            workspace_crdt,
            body_doc_manager,
            wasm_event_registry,
            rust_event_registry,
            crdt_update_subscription: Some(crdt_update_subscription),
            sync_manager,
            diaryx,
        })
    }

    /// Internal helper: build a DiaryxBackend without sync (no CrdtFs, no SyncPlugin).
    #[cfg(not(feature = "sync"))]
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
    // CrdtFs Control (sync feature only)
    // ========================================================================

    /// Enable or disable the CrdtFs decorator.
    #[cfg(feature = "sync")]
    #[wasm_bindgen(js_name = "setCrdtEnabled")]
    pub fn set_crdt_enabled(&self, enabled: bool) {
        self.fs.inner().set_enabled(enabled);
        self.diaryx.fs().inner().set_enabled(enabled);
        log::info!("[DiaryxBackend] CrdtFs enabled: {}", enabled);
    }

    /// Check whether CrdtFs is currently enabled.
    #[cfg(feature = "sync")]
    #[wasm_bindgen(js_name = "isCrdtEnabled")]
    pub fn is_crdt_enabled(&self) -> bool {
        self.diaryx.fs().inner().is_enabled()
    }

    /// No-op stub when sync is disabled.
    #[cfg(not(feature = "sync"))]
    #[wasm_bindgen(js_name = "setCrdtEnabled")]
    pub fn set_crdt_enabled(&self, _enabled: bool) {}

    /// Always returns false when sync is disabled.
    #[cfg(not(feature = "sync"))]
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

    /// Execute a command from a JavaScript object directly.
    ///
    /// This avoids JSON serialization overhead for better performance.
    #[wasm_bindgen(js_name = "executeJs")]
    pub async fn execute_js(&self, command: JsValue) -> std::result::Result<JsValue, JsValue> {
        use diaryx_core::Command;

        // Parse command from JS object
        let cmd: Command = serde_wasm_bindgen::from_value(command)?;

        // Execute the command using the shared Diaryx instance
        // (callbacks were configured once during backend creation)
        let result = self
            .diaryx
            .execute(cmd)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Convert response to JsValue
        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize response: {}", e)))
    }

    // ========================================================================
    // Config (WASM-specific - stored in root index frontmatter)
    // ========================================================================

    /// Get the current configuration from root index frontmatter.
    /// Config keys are stored as `diaryx_*` properties.
    #[wasm_bindgen(js_name = "getConfig")]
    pub fn get_config(&self) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);

            // Find root index - try current directory first ("." for FSA mode)
            let root_path = ws
                .find_root_index_in_dir(Path::new("."))
                .await
                .ok()
                .flatten();

            // Fallback: try "workspace" directory for OPFS mode
            let root_path = match root_path {
                Some(p) => Some(p),
                None => ws
                    .find_root_index_in_dir(Path::new("workspace"))
                    .await
                    .ok()
                    .flatten(),
            };

            let root_path = match root_path {
                Some(p) => p,
                None => {
                    // Return default config if no root found
                    let default = r#"{"default_workspace":"."}"#;
                    return js_sys::JSON::parse(default)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)));
                }
            };

            // Read frontmatter from root index
            match ws.parse_index(&root_path).await {
                Ok(index) => {
                    // Extract diaryx_* keys from extra
                    let mut config = serde_json::Map::new();

                    // Set default_workspace to root index's directory
                    if let Some(parent) = root_path.parent() {
                        let ws_path = if parent.as_os_str().is_empty() {
                            "."
                        } else {
                            &parent.to_string_lossy()
                        };
                        config.insert(
                            "default_workspace".to_string(),
                            serde_json::Value::String(ws_path.to_string()),
                        );
                    }

                    // Extract diaryx_* keys
                    for (key, value) in &index.frontmatter.extra {
                        if let Some(config_key) = key.strip_prefix("diaryx_") {
                            // Convert serde_yaml::Value to serde_json::Value
                            if let Ok(json_str) = serde_yaml::to_string(value) {
                                if let Ok(json_val) =
                                    serde_json::from_str::<serde_json::Value>(&json_str)
                                {
                                    config.insert(config_key.to_string(), json_val);
                                }
                            }
                        }
                    }

                    let config_obj = serde_json::Value::Object(config);
                    let config_str = serde_json::to_string(&config_obj)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))?;

                    js_sys::JSON::parse(&config_str)
                        .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))
                }
                Err(_) => {
                    // Return default config
                    let default = r#"{"default_workspace":"."}"#;
                    js_sys::JSON::parse(default)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))
                }
            }
        })
    }

    /// Save configuration to root index frontmatter.
    /// Config keys are stored as `diaryx_*` properties.
    #[wasm_bindgen(js_name = "saveConfig")]
    pub fn save_config(&self, config_js: JsValue) -> Promise {
        let fs = self.fs.clone();

        future_to_promise(async move {
            let ws = Workspace::new(&*fs);

            // Find root index
            let root_path = ws
                .find_root_index_in_dir(Path::new("."))
                .await
                .ok()
                .flatten();

            // Fallback: try "workspace" directory for OPFS mode
            let root_path = match root_path {
                Some(p) => Some(p),
                None => ws
                    .find_root_index_in_dir(Path::new("workspace"))
                    .await
                    .ok()
                    .flatten(),
            };

            let root_path = match root_path {
                Some(p) if fs.exists(&p).await => p,
                _ => return Err(JsValue::from_str("No root index found to save config")),
            };

            // Parse config from JS
            let config_str = js_sys::JSON::stringify(&config_js)
                .map_err(|e| JsValue::from_str(&format!("Failed to stringify config: {:?}", e)))?;
            let config: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&String::from(config_str))
                    .map_err(|e| JsValue::from_str(&format!("Invalid config JSON: {:?}", e)))?;

            // Read current file
            let content = fs
                .read_to_string(&root_path)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            // Parse frontmatter
            let mut parsed = frontmatter::parse_or_empty(&content)
                .map_err(|e| JsValue::from_str(&format!("Failed to parse frontmatter: {:?}", e)))?;

            // Update diaryx_* keys (skip default_workspace as it's derived)
            for (key, value) in config {
                if key != "default_workspace" {
                    let yaml_key = format!("diaryx_{}", key);
                    // Convert JSON value to YAML
                    let yaml_str = serde_json::to_string(&value)
                        .map_err(|e| JsValue::from_str(&format!("JSON error: {:?}", e)))?;
                    let yaml_val: serde_yaml::Value = serde_yaml::from_str(&yaml_str)
                        .map_err(|e| JsValue::from_str(&format!("YAML error: {:?}", e)))?;
                    parsed.frontmatter.insert(yaml_key, yaml_val);
                }
            }

            // Serialize and write back
            let new_content = frontmatter::serialize(&parsed.frontmatter, &parsed.body)
                .map_err(|e| JsValue::from_str(&format!("Failed to serialize: {:?}", e)))?;

            fs.write_file(&root_path, &new_content)
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;

            Ok(JsValue::UNDEFINED)
        })
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

    // ========================================================================
    // Sync Client API (sync feature only)
    // ========================================================================

    /// Create a new sync client for the given server and workspace.
    #[cfg(feature = "sync")]
    #[wasm_bindgen(js_name = "createSyncClient")]
    pub fn create_sync_client(
        &self,
        server_url: String,
        workspace_id: String,
        auth_token: Option<String>,
    ) -> crate::wasm_sync_client::WasmSyncClient {
        let session_config = SyncSessionConfig {
            workspace_id: workspace_id.clone(),
            write_to_disk: true,
        };

        log::info!(
            "[DiaryxBackend] Creating WasmSyncClient for workspace: {}",
            workspace_id
        );

        crate::wasm_sync_client::WasmSyncClient::new(
            server_url,
            workspace_id,
            auth_token,
            session_config,
            Arc::clone(&self.sync_manager),
        )
    }

    /// Check if this backend has native sync support.
    #[wasm_bindgen(js_name = "hasNativeSync")]
    pub fn has_native_sync(&self) -> bool {
        false
    }

    // ========================================================================
    // Import Parsing
    // ========================================================================

    /// Parse a Day One export (ZIP or JSON) and return entries as JSON.
    ///
    /// Auto-detects the format: ZIP files (with media directories) have
    /// attachments populated with binary data. Plain JSON files are parsed
    /// with empty attachment data (backward compatible).
    ///
    /// ## Example
    /// ```javascript
    /// const bytes = new Uint8Array(await file.arrayBuffer());
    /// const result = backend.parseDayOneJson(bytes);
    /// const { entries, errors } = JSON.parse(result);
    /// ```
    #[wasm_bindgen(js_name = "parseDayOneJson")]
    pub fn parse_dayone_json(&self, bytes: &[u8]) -> std::result::Result<String, JsValue> {
        let result = diaryx_core::import::dayone::parse_dayone_auto(bytes);

        let mut entries = Vec::new();
        let mut errors = Vec::new();
        for r in result.entries {
            match r {
                Ok(entry) => entries.push(entry),
                Err(e) => errors.push(e),
            }
        }

        #[derive(serde::Serialize)]
        struct ParseResult {
            entries: Vec<diaryx_core::import::ImportedEntry>,
            errors: Vec<String>,
            journal_name: Option<String>,
        }

        serde_json::to_string(&ParseResult {
            entries,
            errors,
            journal_name: result.journal_name,
        })
        .map_err(|e| JsValue::from_str(&format!("Failed to serialize parse result: {e}")))
    }

    /// Parse a single markdown file and return the entry as JSON.
    ///
    /// Takes the raw bytes of a `.md` file and its filename, and returns
    /// a JSON-serialized `ImportedEntry`.
    ///
    /// ## Example
    /// ```javascript
    /// const bytes = new Uint8Array(await file.arrayBuffer());
    /// const entryJson = backend.parseMarkdownFile(bytes, file.name);
    /// const entry = JSON.parse(entryJson);
    /// ```
    #[wasm_bindgen(js_name = "parseMarkdownFile")]
    pub fn parse_markdown_file(
        &self,
        bytes: &[u8],
        filename: &str,
    ) -> std::result::Result<String, JsValue> {
        let entry = diaryx_core::import::markdown::parse_markdown_file(bytes, filename)
            .map_err(|e| JsValue::from_str(&e))?;

        serde_json::to_string(&entry)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize entry: {e}")))
    }
}
