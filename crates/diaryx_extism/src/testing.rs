//! Test harness for integration-testing Extism guest plugins.
//!
//! Feature-gated behind `testing`. Provides [`PluginTestHarness`] for loading
//! a `.wasm` plugin with mock host functions and exercising its exports.
//!
//! # Example
//!
//! ```rust,ignore
//! use diaryx_extism::testing::PluginTestHarness;
//!
//! #[tokio::test]
//! async fn test_manifest() {
//!     let harness = PluginTestHarness::load("target/wasm32-wasip1/release/my_plugin.wasm")
//!         .expect("Failed to load plugin");
//!     let manifest = harness.manifest();
//!     assert_eq!(manifest.id.0, "diaryx.myplugin");
//! }
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value as JsonValue;
use serde_json::json;

use diaryx_core::fs::{RealFileSystem, SyncToAsyncFs};
use diaryx_core::plugin::permissions::PermissionType;
use diaryx_core::plugin::{
    FileCreatedEvent, FileDeletedEvent, FileMovedEvent, FilePlugin, FileSavedEvent, Plugin,
    PluginContext, PluginError, PluginId, PluginManifest, WorkspaceOpenedEvent, WorkspacePlugin,
};

use crate::host_fns::*;
use crate::loader::load_plugin_from_wasm;

// ============================================================================
// Recording implementations for test assertions
// ============================================================================

/// An event emitter that records all emitted events for later assertion.
pub struct RecordingEventEmitter {
    events: Mutex<Vec<String>>,
}

impl RecordingEventEmitter {
    /// Create a new recording emitter.
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    /// Get all recorded event JSON strings.
    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    /// Get recorded events parsed as JSON values.
    pub fn events_json(&self) -> Vec<JsonValue> {
        self.events()
            .into_iter()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect()
    }

    /// Clear recorded events.
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }
}

impl Default for RecordingEventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter for RecordingEventEmitter {
    fn emit(&self, event_json: &str) {
        self.events.lock().unwrap().push(event_json.to_string());
    }
}

/// A recorded storage operation.
#[derive(Debug, Clone)]
pub enum StorageOp {
    /// A `get` call with the key.
    Get(String),
    /// A `set` call with the key and data.
    Set(String, Vec<u8>),
    /// A `delete` call with the key.
    Delete(String),
}

/// A storage implementation that records all operations and stores data in memory.
pub struct RecordingStorage {
    data: Mutex<HashMap<String, Vec<u8>>>,
    ops: Mutex<Vec<StorageOp>>,
}

impl RecordingStorage {
    /// Create a new empty recording storage.
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
            ops: Mutex::new(Vec::new()),
        }
    }

    /// Pre-populate storage with data (builder pattern).
    pub fn with_data(self, key: &str, value: &[u8]) -> Self {
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
        self
    }

    /// Get all recorded operations.
    pub fn ops(&self) -> Vec<StorageOp> {
        self.ops.lock().unwrap().clone()
    }

    /// Snapshot the current in-memory contents.
    pub fn data_snapshot(&self) -> HashMap<String, Vec<u8>> {
        self.data.lock().unwrap().clone()
    }
}

impl Default for RecordingStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginStorage for RecordingStorage {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.ops
            .lock()
            .unwrap()
            .push(StorageOp::Get(key.to_string()));
        self.data.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, data: &[u8]) {
        self.ops
            .lock()
            .unwrap()
            .push(StorageOp::Set(key.to_string(), data.to_vec()));
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
    }

    fn delete(&self, key: &str) {
        self.ops
            .lock()
            .unwrap()
            .push(StorageOp::Delete(key.to_string()));
        self.data.lock().unwrap().remove(key);
    }
}

/// An allow-all permission checker for testing.
///
/// Permits every host function call without restrictions.
pub struct AllowAllPermissionChecker;

impl PermissionChecker for AllowAllPermissionChecker {
    fn check_permission(
        &self,
        _plugin_id: &str,
        _permission_type: PermissionType,
        _target: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

// ============================================================================
// Test harness builder and struct
// ============================================================================

/// Builder for configuring a [`PluginTestHarness`].
///
/// # Example
///
/// ```rust,ignore
/// let storage = Arc::new(RecordingStorage::new());
/// let emitter = Arc::new(RecordingEventEmitter::new());
///
/// let harness = PluginTestHarnessBuilder::new("path/to/plugin.wasm")
///     .with_storage(storage.clone())
///     .with_event_emitter(emitter.clone())
///     .with_workspace_root("/tmp/test-workspace")
///     .build()
///     .expect("Failed to load plugin");
/// ```
pub struct PluginTestHarnessBuilder {
    wasm_path: PathBuf,
    storage: Option<Arc<dyn PluginStorage>>,
    event_emitter: Option<Arc<dyn EventEmitter>>,
    permission_checker: Option<Arc<dyn PermissionChecker>>,
    workspace_root: Option<PathBuf>,
    runtime_context_provider: Option<Arc<dyn RuntimeContextProvider>>,
    namespace_provider: Option<Arc<dyn NamespaceProvider>>,
}

struct StaticRuntimeContextProvider {
    value: JsonValue,
}

impl StaticRuntimeContextProvider {
    fn new(value: JsonValue) -> Self {
        Self { value }
    }
}

impl RuntimeContextProvider for StaticRuntimeContextProvider {
    fn get_context(&self, _plugin_id: &str) -> JsonValue {
        self.value.clone()
    }
}

impl PluginTestHarnessBuilder {
    /// Start building a test harness for the given WASM file.
    pub fn new(wasm_path: impl Into<PathBuf>) -> Self {
        Self {
            wasm_path: wasm_path.into(),
            storage: None,
            event_emitter: None,
            permission_checker: None,
            workspace_root: None,
            runtime_context_provider: None,
            namespace_provider: None,
        }
    }

    /// Use a custom storage implementation (e.g., [`RecordingStorage`]).
    pub fn with_storage(mut self, storage: Arc<dyn PluginStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Use a custom event emitter (e.g., [`RecordingEventEmitter`]).
    pub fn with_event_emitter(mut self, emitter: Arc<dyn EventEmitter>) -> Self {
        self.event_emitter = Some(emitter);
        self
    }

    /// Use a custom permission checker.
    pub fn with_permission_checker(mut self, checker: Arc<dyn PermissionChecker>) -> Self {
        self.permission_checker = Some(checker);
        self
    }

    /// Set the workspace root for the test context.
    pub fn with_workspace_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(root.into());
        self
    }

    /// Use a custom runtime context provider.
    pub fn with_runtime_context_provider(
        mut self,
        provider: Arc<dyn RuntimeContextProvider>,
    ) -> Self {
        self.runtime_context_provider = Some(provider);
        self
    }

    /// Use a fixed runtime context JSON payload.
    pub fn with_runtime_context(mut self, context: JsonValue) -> Self {
        self.runtime_context_provider = Some(Arc::new(StaticRuntimeContextProvider::new(context)));
        self
    }

    /// Use a custom namespace provider.
    pub fn with_namespace_provider(mut self, provider: Arc<dyn NamespaceProvider>) -> Self {
        self.namespace_provider = Some(provider);
        self
    }

    /// Build the test harness, loading the WASM plugin.
    pub fn build(self) -> Result<PluginTestHarness, String> {
        let fs = Arc::new(SyncToAsyncFs::new(RealFileSystem));
        let runtime_context_provider = self.runtime_context_provider.unwrap_or_else(|| {
            if let Some(root) = &self.workspace_root {
                Arc::new(StaticRuntimeContextProvider::new(json!({
                    "current_workspace": {
                        "path": root.to_string_lossy(),
                    }
                }))) as Arc<dyn RuntimeContextProvider>
            } else {
                Arc::new(NoopRuntimeContextProvider)
            }
        });
        let host_context = Arc::new(HostContext {
            fs,
            storage: self.storage.unwrap_or_else(|| Arc::new(NoopStorage)),
            secret_store: Arc::new(NoopSecretStore),
            event_emitter: self
                .event_emitter
                .unwrap_or_else(|| Arc::new(NoopEventEmitter)),
            plugin_id: String::new(),
            plugin_id_locked: false,
            permission_checker: Some(
                self.permission_checker
                    .unwrap_or_else(|| Arc::new(AllowAllPermissionChecker)),
            ),
            file_provider: Arc::new(NoopFileProvider),
            ws_bridge: Arc::new(NoopWebSocketBridge),
            plugin_command_bridge: Arc::new(NoopPluginCommandBridge),
            runtime_context_provider,
            namespace_provider: self
                .namespace_provider
                .unwrap_or_else(|| Arc::new(crate::host_fns::NoopNamespaceProvider)),
            plugin_command_depth: 0,
            storage_quota_bytes: crate::host_fns::DEFAULT_STORAGE_QUOTA_BYTES,
        });

        let adapter = load_plugin_from_wasm(&self.wasm_path, host_context, None)
            .map_err(|e| format!("Failed to load plugin: {e}"))?;

        Ok(PluginTestHarness {
            adapter: Arc::new(adapter),
            workspace_root: self.workspace_root,
        })
    }
}

/// Test harness for exercising a loaded WASM plugin.
///
/// Wraps an [`ExtismPluginAdapter`](crate::adapter::ExtismPluginAdapter) and
/// provides convenience methods for sending events, dispatching commands,
/// and managing configuration during tests.
pub struct PluginTestHarness {
    adapter: Arc<crate::adapter::ExtismPluginAdapter>,
    workspace_root: Option<PathBuf>,
}

impl PluginTestHarness {
    /// Convenience constructor with all default (noop) host functions
    /// and allow-all permissions.
    pub fn load(wasm_path: impl Into<PathBuf>) -> Result<Self, String> {
        PluginTestHarnessBuilder::new(wasm_path).build()
    }

    /// Get the plugin's manifest.
    pub fn manifest(&self) -> PluginManifest {
        self.adapter.manifest()
    }

    /// Get the plugin's ID.
    pub fn plugin_id(&self) -> PluginId {
        self.adapter.id()
    }

    /// Initialize the plugin with a test context.
    pub async fn init(&self) -> Result<(), PluginError> {
        let ctx = PluginContext::new(
            self.workspace_root.clone(),
            diaryx_core::link_parser::LinkFormat::default(),
        );
        self.adapter.init(&ctx).await
    }

    /// Send a command and get the response.
    pub async fn command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        self.adapter.handle_command(cmd, params).await
    }

    /// Send a workspace-opened event.
    pub async fn send_workspace_opened(&self, workspace_root: PathBuf) {
        self.adapter
            .on_workspace_opened(&WorkspaceOpenedEvent { workspace_root })
            .await;
    }

    /// Send a file-saved event.
    pub async fn send_file_saved(&self, path: &str) {
        self.adapter
            .on_file_saved(&FileSavedEvent {
                path: path.to_string(),
            })
            .await;
    }

    /// Send a file-created event.
    pub async fn send_file_created(&self, path: &str) {
        self.adapter
            .on_file_created(&FileCreatedEvent {
                path: path.to_string(),
            })
            .await;
    }

    /// Send a file-deleted event.
    pub async fn send_file_deleted(&self, path: &str) {
        self.adapter
            .on_file_deleted(&FileDeletedEvent {
                path: path.to_string(),
            })
            .await;
    }

    /// Send a file-moved event.
    pub async fn send_file_moved(&self, old_path: &str, new_path: &str) {
        self.adapter
            .on_file_moved(&FileMovedEvent {
                old_path: old_path.to_string(),
                new_path: new_path.to_string(),
            })
            .await;
    }

    /// Get plugin config.
    pub async fn get_config(&self) -> Option<JsonValue> {
        self.adapter.get_config().await
    }

    /// Set plugin config.
    pub async fn set_config(&self, config: JsonValue) -> Result<(), PluginError> {
        self.adapter.set_config(config).await
    }

    /// Call a raw guest export (for non-standard exports like `render_content`).
    pub fn call_raw(&self, func: &str, input: &str) -> Result<String, PluginError> {
        self.adapter.call_guest(func, input)
    }
}
