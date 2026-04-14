//! Integration tests for the diaryx-sync Extism plugin.
//!
//! These tests load the pre-built WASM module via `PluginTestHarness` and
//! exercise the plugin's exports through the Extism runtime.
//!
//! Prerequisites: `cargo build --target wasm32-unknown-unknown --release`

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use diaryx_core::plugin::manifest::{PluginCapability, UiContribution};
use diaryx_extism::testing::*;
use diaryx_extism::{NamespaceEntry, NamespaceObjectMeta, NamespaceProvider};
use diaryx_sync_extism::sync_manifest::SyncManifest;
use serde_json::{Value as JsonValue, json};

const WASM_PATH: &str = "target/wasm32-unknown-unknown/release/diaryx_sync_extism.wasm";

/// Early-return if the WASM file hasn't been built.
macro_rules! require_wasm {
    () => {
        if !std::path::Path::new(WASM_PATH).exists() {
            eprintln!(
                "Skipping: WASM not built. Run: cargo build --target wasm32-unknown-unknown --release"
            );
            return;
        }
    };
}

fn load_sync_plugin() -> PluginTestHarness {
    PluginTestHarness::load(WASM_PATH).expect("Failed to load sync plugin WASM")
}

fn load_with_storage(storage: Arc<RecordingStorage>) -> PluginTestHarness {
    PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage)
        .build()
        .expect("Failed to load sync plugin WASM")
}

fn load_with_storage_and_emitter(
    storage: Arc<RecordingStorage>,
    emitter: Arc<RecordingEventEmitter>,
) -> PluginTestHarness {
    PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage)
        .with_event_emitter(emitter)
        .build()
        .expect("Failed to load sync plugin WASM")
}

#[derive(Default)]
struct MockNamespaceProvider {
    namespaces: Mutex<Vec<NamespaceEntry>>,
    objects: Mutex<BTreeMap<(String, String), Vec<u8>>>,
    put_errors: Mutex<HashMap<String, String>>,
    delete_errors: Mutex<HashMap<String, Vec<String>>>,
    list_namespaces_error: Mutex<Option<String>>,
    next_namespace_id: Mutex<u64>,
}

impl MockNamespaceProvider {
    fn new() -> Self {
        Self::default()
    }

    fn seed_namespace(&self, id: &str, name: &str) {
        self.namespaces.lock().unwrap().push(NamespaceEntry {
            id: id.to_string(),
            owner_user_id: "user-1".to_string(),
            created_at: 1,
            metadata: Some(json!({ "name": name, "provider": "diaryx.sync" })),
        });
    }

    fn seed_object(&self, namespace_id: &str, key: &str, body: &[u8]) {
        self.objects
            .lock()
            .unwrap()
            .insert((namespace_id.to_string(), key.to_string()), body.to_vec());
    }

    fn fail_put_for_key(&self, key: &str, error: &str) {
        self.put_errors
            .lock()
            .unwrap()
            .insert(key.to_string(), error.to_string());
    }

    fn push_delete_error(&self, key: &str, error: &str) {
        self.delete_errors
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .push(error.to_string());
    }
}

impl NamespaceProvider for MockNamespaceProvider {
    fn create_namespace(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Result<NamespaceEntry, String> {
        let mut next_id = self.next_namespace_id.lock().unwrap();
        *next_id += 1;
        let entry = NamespaceEntry {
            id: format!("ns-{}", *next_id),
            owner_user_id: "user-1".to_string(),
            created_at: 1,
            metadata: metadata.cloned(),
        };
        self.namespaces.lock().unwrap().push(entry.clone());
        Ok(entry)
    }

    fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        _mime_type: &str,
        _audience: Option<&str>,
    ) -> Result<(), String> {
        if let Some(error) = self.put_errors.lock().unwrap().get(key).cloned() {
            return Err(error);
        }
        self.objects
            .lock()
            .unwrap()
            .insert((ns_id.to_string(), key.to_string()), bytes.to_vec());
        Ok(())
    }

    fn get_object(&self, ns_id: &str, key: &str) -> Result<Vec<u8>, String> {
        self.objects
            .lock()
            .unwrap()
            .get(&(ns_id.to_string(), key.to_string()))
            .cloned()
            .ok_or_else(|| "404 not found".to_string())
    }

    fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String> {
        if let Some(errors) = self.delete_errors.lock().unwrap().get_mut(key) {
            if !errors.is_empty() {
                return Err(errors.remove(0));
            }
        }
        self.objects
            .lock()
            .unwrap()
            .remove(&(ns_id.to_string(), key.to_string()));
        Ok(())
    }

    fn list_objects(
        &self,
        ns_id: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<NamespaceObjectMeta>, String> {
        let prefix = prefix.unwrap_or_default();
        let mut objects = self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|((namespace_id, key), _)| namespace_id == ns_id && key.starts_with(prefix))
            .map(|((namespace_id, key), body)| NamespaceObjectMeta {
                namespace_id: Some(namespace_id.clone()),
                key: key.clone(),
                r2_key: None,
                audience: None,
                mime_type: Some("application/octet-stream".to_string()),
                size_bytes: Some(body.len() as u64),
                updated_at: Some(1),
                content_hash: None,
            })
            .collect::<Vec<_>>();
        objects.sort_by(|a, b| a.key.cmp(&b.key));
        let offset = offset.unwrap_or(0) as usize;
        let limit = limit.unwrap_or(u32::MAX) as usize;
        Ok(objects.into_iter().skip(offset).take(limit).collect())
    }

    fn sync_audience(&self, _ns_id: &str, _audience: &str, _access: &str) -> Result<(), String> {
        Ok(())
    }

    fn send_audience_email(
        &self,
        _ns_id: &str,
        _audience: &str,
        _subject: &str,
        _reply_to: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        Ok(json!({ "ok": true }))
    }

    fn list_namespaces(&self) -> Result<Vec<NamespaceEntry>, String> {
        if let Some(error) = self.list_namespaces_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(self.namespaces.lock().unwrap().clone())
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "diaryx-sync-extism-{label}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

fn write_workspace_file(root_dir: &Path, relative_path: &str, contents: &str) {
    let path = root_dir.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("workspace parent directories should exist");
    }
    std::fs::write(path, contents).expect("workspace file should be written");
}

fn create_workspace(root_filename: &str, root_contents: &str, files: &[(&str, &str)]) -> PathBuf {
    let root_dir = unique_temp_dir(root_filename.trim_end_matches(".md"));
    write_workspace_file(&root_dir, root_filename, root_contents);
    for (relative_path, contents) in files {
        write_workspace_file(&root_dir, relative_path, contents);
    }
    root_dir.join(root_filename)
}

fn load_with_storage_workspace_and_namespace(
    storage: Arc<RecordingStorage>,
    workspace_root: &Path,
    namespace_provider: Arc<dyn NamespaceProvider>,
    runtime_context: Option<JsonValue>,
) -> PluginTestHarness {
    let mut builder = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage)
        .with_workspace_root(workspace_root);
    if let Some(context) = runtime_context {
        builder = builder.with_runtime_context(context);
    }
    builder
        .with_namespace_provider(namespace_provider)
        .build()
        .expect("Failed to load sync plugin WASM")
}

fn saved_manifest(storage: &RecordingStorage) -> SyncManifest {
    let data = storage
        .data_snapshot()
        .into_iter()
        .find(|(key, _)| key.ends_with("sync_manifest"))
        .map(|(_, value)| value)
        .expect("sync manifest should be stored");
    serde_json::from_slice(&data).expect("sync manifest should deserialize")
}

// ============================================================================
// Category 1: Manifest & Metadata
// ============================================================================

#[test]
fn manifest_has_correct_id_and_name() {
    require_wasm!();
    let harness = load_sync_plugin();
    let manifest = harness.manifest();
    assert_eq!(manifest.id.0, "diaryx.sync");
    assert_eq!(manifest.name, "Sync");
}

#[test]
fn manifest_declares_expected_capabilities() {
    require_wasm!();
    let harness = load_sync_plugin();
    let manifest = harness.manifest();

    let has_file_events = manifest
        .capabilities
        .iter()
        .any(|c| matches!(c, PluginCapability::FileEvents));
    let has_workspace_events = manifest
        .capabilities
        .iter()
        .any(|c| matches!(c, PluginCapability::WorkspaceEvents));
    let has_custom_commands = manifest
        .capabilities
        .iter()
        .any(|c| matches!(c, PluginCapability::CustomCommands { .. }));

    assert!(has_file_events, "Missing FileEvents capability");
    assert!(has_workspace_events, "Missing WorkspaceEvents capability");
    assert!(has_custom_commands, "Missing CustomCommands capability");
}

#[test]
fn manifest_declares_commands() {
    require_wasm!();
    let harness = load_sync_plugin();
    let manifest = harness.manifest();

    // Commands are embedded in CustomCommands capability
    let commands: Vec<&str> = manifest
        .capabilities
        .iter()
        .filter_map(|c| match c {
            PluginCapability::CustomCommands { commands } => Some(commands),
            _ => None,
        })
        .flatten()
        .map(|s| s.as_str())
        .collect();

    for expected in [
        "GetSyncStatus",
        "SyncPush",
        "SyncPull",
        "Sync",
        "SyncStatus",
        "GetProviderStatus",
        "get_config",
        "set_config",
        "ListRemoteWorkspaces",
        "LinkWorkspace",
    ] {
        assert!(
            commands.contains(&expected),
            "Missing command: {expected}. Got: {commands:?}"
        );
    }
}

#[test]
fn manifest_declares_ui_contributions() {
    require_wasm!();
    let harness = load_sync_plugin();
    let manifest = harness.manifest();

    assert!(
        !manifest.ui.is_empty(),
        "Manifest should declare UI contributions"
    );

    let has_settings_tab = manifest
        .ui
        .iter()
        .any(|ui| matches!(ui, UiContribution::SettingsTab { id, .. } if id == "sync-settings"));
    assert!(has_settings_tab, "Should have sync-settings SettingsTab");

    let has_status_bar = manifest
        .ui
        .iter()
        .any(|ui| matches!(ui, UiContribution::StatusBarItem { .. }));
    assert!(has_status_bar, "Should have StatusBarItem");
}

// ============================================================================
// Category 2: Init & Lifecycle
// ============================================================================

#[tokio::test]
async fn init_succeeds() {
    require_wasm!();
    let harness = load_sync_plugin();
    let result = harness.init().await;
    assert!(result.is_ok(), "Init should succeed: {result:?}");
}

#[tokio::test]
async fn init_with_workspace_root() {
    require_wasm!();
    let tmp = std::env::temp_dir().join("diaryx-test-workspace");
    let _ = std::fs::create_dir_all(&tmp);

    let harness = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_workspace_root(&tmp)
        .build()
        .expect("Failed to load");

    let result = harness.init().await;
    assert!(
        result.is_ok(),
        "Init with workspace root should succeed: {result:?}"
    );
}

#[tokio::test]
async fn shutdown_persists_sync_manifest() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let harness = load_with_storage(storage.clone());

    harness.init().await.expect("init should succeed");

    // Send a file event to create manifest state
    harness.send_file_saved("docs/test.md").await;

    // Call shutdown to persist state
    let _ = harness.call_raw("shutdown", "");

    // Check that sync_manifest was written to storage
    let ops = storage.ops();
    let has_manifest_set = ops
        .iter()
        .any(|op| matches!(op, StorageOp::Set(key, _) if key.ends_with("sync_manifest")));
    assert!(
        has_manifest_set,
        "Shutdown should persist sync_manifest to storage. Ops: {ops:?}"
    );
}

// ============================================================================
// Category 3: Config Management
// ============================================================================

#[tokio::test]
async fn get_config_returns_defaults_when_empty() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("get_config", json!({}))
        .await
        .expect("get_config should return Some")
        .expect("get_config should succeed");

    // Default config should be a JSON object
    assert!(result.is_object(), "get_config should return a JSON object");
}

#[tokio::test]
async fn set_config_stores_server_url() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let harness = load_with_storage(storage.clone());
    harness.init().await.expect("init");

    // Set server_url
    harness
        .command(
            "set_config",
            json!({ "server_url": "https://test.example.com" }),
        )
        .await
        .expect("set_config should return Some")
        .expect("set_config should succeed");

    // Read it back
    let config = harness
        .command("get_config", json!({}))
        .await
        .expect("get_config should return Some")
        .expect("get_config should succeed");

    assert_eq!(
        config.get("server_url").and_then(|v| v.as_str()),
        Some("https://test.example.com"),
        "server_url should be stored. Got: {config}"
    );
}

#[tokio::test]
async fn set_config_roundtrip() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let harness = load_with_storage(storage);
    harness.init().await.expect("init");

    // Set all three fields
    harness
        .command(
            "set_config",
            json!({
                "server_url": "https://sync.example.com",
                "auth_token": "test-token-123",
                "workspace_id": "ws-abc-456"
            }),
        )
        .await
        .expect("Some")
        .expect("set_config should succeed");

    // Read back
    let config = harness
        .command("get_config", json!({}))
        .await
        .expect("Some")
        .expect("get_config should succeed");

    assert_eq!(
        config.get("server_url").and_then(|v| v.as_str()),
        Some("https://sync.example.com")
    );
    assert_eq!(
        config.get("auth_token").and_then(|v| v.as_str()),
        Some("test-token-123"),
        "auth_token should be stored in config"
    );
    assert_eq!(
        config.get("workspace_id").and_then(|v| v.as_str()),
        Some("ws-abc-456")
    );
}

#[tokio::test]
async fn set_config_null_clears_fields() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let harness = load_with_storage(storage);
    harness.init().await.expect("init");

    // Set a field
    harness
        .command(
            "set_config",
            json!({ "server_url": "https://sync.example.com" }),
        )
        .await
        .expect("Some")
        .expect("set_config");

    // Clear it with null
    harness
        .command("set_config", json!({ "server_url": null }))
        .await
        .expect("Some")
        .expect("set_config should succeed");

    // Verify cleared
    let config = harness
        .command("get_config", json!({}))
        .await
        .expect("Some")
        .expect("get_config");

    let server_url = config.get("server_url");
    assert!(
        server_url.is_none() || server_url == Some(&JsonValue::Null),
        "server_url should be cleared after setting null. Got: {config}"
    );
}

// ============================================================================
// Category 4: Status Commands
// ============================================================================

#[tokio::test]
async fn get_sync_status_returns_idle_initially() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("GetSyncStatus", json!({}))
        .await
        .expect("GetSyncStatus should return Some")
        .expect("GetSyncStatus should succeed");

    assert_eq!(
        result.get("state").and_then(|v| v.as_str()),
        Some("synced"),
        "Initial sync status should be synced (no dirty files). Got: {result}"
    );
    assert_eq!(
        result.get("label").and_then(|v| v.as_str()),
        Some("Not linked"),
        "Initial sync label should be Not linked. Got: {result}"
    );
}

#[tokio::test]
async fn get_provider_status_not_ready_without_config() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("GetProviderStatus", json!({}))
        .await
        .expect("GetProviderStatus should return Some")
        .expect("GetProviderStatus should succeed");

    assert_eq!(
        result.get("ready").and_then(|v| v.as_bool()),
        Some(false),
        "Provider should not be ready without credentials. Got: {result}"
    );
}

#[tokio::test]
async fn get_provider_status_ready_with_credentials() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.seed_namespace("ns-1", "Workspace");
    let harness = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage)
        .with_namespace_provider(namespace_provider)
        .build()
        .expect("Failed to load sync plugin WASM");
    harness.init().await.expect("init");

    // Configure server and auth
    harness
        .command(
            "set_config",
            json!({
                "server_url": "https://sync.example.com",
                "auth_token": "valid-token"
            }),
        )
        .await
        .expect("Some")
        .expect("set_config");

    let result = harness
        .command("GetProviderStatus", json!({}))
        .await
        .expect("Some")
        .expect("GetProviderStatus should succeed");

    assert_eq!(
        result.get("ready").and_then(|v| v.as_bool()),
        Some(true),
        "Provider should be ready when the namespace probe succeeds. Got: {result}"
    );
}

#[tokio::test]
async fn get_provider_status_ready_with_cookie_style_runtime_context() {
    require_wasm!();
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.seed_namespace("ns-1", "Workspace");
    let harness = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_runtime_context(json!({
            "server_url": "https://sync.example.com"
        }))
        .with_namespace_provider(namespace_provider)
        .build()
        .expect("Failed to load sync plugin WASM");
    harness.init().await.expect("init");

    let result = harness
        .command("GetProviderStatus", json!({}))
        .await
        .expect("Some")
        .expect("GetProviderStatus should succeed");

    assert_eq!(
        result.get("ready").and_then(|v| v.as_bool()),
        Some(true),
        "Provider should be ready when the host session can list namespaces. Got: {result}"
    );
}

// ============================================================================
// Category 5: CRDT State Commands
// ============================================================================

#[tokio::test]
async fn sync_status_initially_synced() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("SyncStatus", json!({}))
        .await
        .expect("SyncStatus should return Some")
        .expect("SyncStatus should succeed");

    assert_eq!(
        result.get("dirty_count").and_then(|v| v.as_u64()),
        Some(0),
        "Initial dirty_count should be 0. Got: {result}"
    );
}

#[tokio::test]
async fn sync_push_fails_without_namespace() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("SyncPush", json!({}))
        .await
        .expect("SyncPush should return Some");

    assert!(
        result.is_err(),
        "SyncPush should fail without namespace. Got: {result:?}"
    );
}

#[tokio::test]
async fn sync_pull_fails_without_namespace() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("SyncPull", json!({}))
        .await
        .expect("SyncPull should return Some");

    assert!(
        result.is_err(),
        "SyncPull should fail without namespace. Got: {result:?}"
    );
}

#[tokio::test]
async fn file_events_mark_manifest_dirty() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    // Send file_created events
    harness.send_file_created("docs/new-entry.md").await;
    harness.send_file_saved("docs/existing-entry.md").await;

    // Check status shows dirty files
    let result = harness
        .command("SyncStatus", json!({}))
        .await
        .expect("SyncStatus should return Some")
        .expect("SyncStatus should succeed");

    let dirty = result
        .get("dirty_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        dirty > 0,
        "After file events, dirty_count should be > 0. Got: {result}"
    );
}

#[tokio::test]
async fn file_deleted_event_records_pending_delete() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    // Create then delete a file
    harness.send_file_created("docs/temp.md").await;
    harness.send_file_deleted("docs/temp.md").await;

    let result = harness
        .command("SyncStatus", json!({}))
        .await
        .expect("SyncStatus should return Some")
        .expect("SyncStatus should succeed");

    let pending = result
        .get("pending_deletes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        pending > 0,
        "After file delete, pending_deletes should be > 0. Got: {result}"
    );
}

// ============================================================================
// Category 6: Component HTML
// ============================================================================

#[tokio::test]
async fn get_component_html_unknown_returns_error() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command(
            "get_component_html",
            json!({ "component_id": "nonexistent.component" }),
        )
        .await
        .expect("Should return Some");

    assert!(
        result.is_err(),
        "Unknown component ID should return an error. Got: {result:?}"
    );
}

// ============================================================================
// Category 7: Network-Dependent Error Paths
// ============================================================================

#[tokio::test]
async fn list_remote_workspaces_fails_without_server() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("ListRemoteWorkspaces", json!({}))
        .await
        .expect("Should return Some");

    assert!(
        result.is_err(),
        "ListRemoteWorkspaces should fail without server config. Got: {result:?}"
    );
}

#[tokio::test]
async fn link_workspace_fails_without_server() {
    require_wasm!();
    let harness = load_sync_plugin();
    harness.init().await.expect("init");

    let result = harness
        .command("LinkWorkspace", json!({}))
        .await
        .expect("Should return Some");

    assert!(
        result.is_err(),
        "LinkWorkspace should fail without server config. Got: {result:?}"
    );
}

// ============================================================================
// Category 8: Events
// ============================================================================

#[tokio::test]
async fn file_created_event_does_not_crash() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    // Should not panic
    harness.send_file_created("docs/new-entry.md").await;
}

#[tokio::test]
async fn file_saved_event_does_not_crash() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    // Should not panic
    harness.send_file_saved("docs/existing-entry.md").await;
}

#[tokio::test]
async fn file_deleted_event_does_not_crash() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    // Should not panic
    harness.send_file_deleted("docs/removed-entry.md").await;
}

#[tokio::test]
async fn file_deleted_event_creates_pending_delete_entry() {
    require_wasm!();
    let storage = Arc::new(RecordingStorage::new());
    let emitter = Arc::new(RecordingEventEmitter::new());
    let harness = load_with_storage_and_emitter(storage, emitter);
    harness.init().await.expect("init");

    harness.send_file_deleted("docs/removed-entry.md").await;

    let status = harness
        .command("SyncStatus", json!({}))
        .await
        .expect("SyncStatus should return Some")
        .expect("SyncStatus should succeed");

    let pending = status
        .get("pending_deletes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        pending > 0,
        "deleted file should create a pending delete record. Got: {status}"
    );
}

#[tokio::test]
async fn file_moved_absolute_paths_record_relative_manifest_entries() {
    require_wasm!();
    let root_index = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - new.md\n---\n\n# Root\n",
        &[],
    );
    let storage = Arc::new(RecordingStorage::new());
    let harness = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_storage(storage.clone())
        .with_workspace_root(&root_index)
        .build()
        .expect("Failed to load");
    harness.init().await.expect("init");

    let root_dir = root_index.parent().expect("workspace dir should exist");
    let old_path = root_dir.join("old.md");
    let new_path = root_dir.join("new.md");
    harness
        .send_file_moved(
            old_path.to_str().expect("old path should be utf-8"),
            new_path.to_str().expect("new path should be utf-8"),
        )
        .await;

    let _ = harness.call_raw("shutdown", "");
    let manifest = saved_manifest(&storage);

    assert_eq!(
        manifest
            .pending_deletes
            .iter()
            .map(|delete| delete.path.as_str())
            .collect::<Vec<_>>(),
        vec!["files/old.md"],
    );
    assert!(
        manifest.files.contains_key("files/new.md"),
        "new file should be tracked with a relative manifest key: {:?}",
        manifest.files.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn sync_push_retries_remote_delete_after_transient_failure() {
    require_wasm!();
    let root_index = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - new.md\n---\n\n# Root\n",
        &[(
            "new.md",
            "---\ntitle: New\npart_of: index.md\n---\n\n# New\n",
        )],
    );
    let storage = Arc::new(RecordingStorage::new());
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.seed_object("ns-1", "files/old.md", b"# Old\n");
    namespace_provider.push_delete_error("files/old.md", "network timeout");
    let harness = load_with_storage_workspace_and_namespace(
        storage.clone(),
        &root_index,
        namespace_provider,
        None,
    );
    harness.init().await.expect("init");
    harness
        .command("set_config", json!({ "workspace_id": "ns-1" }))
        .await
        .expect("Some")
        .expect("set_config should succeed");

    let root_dir = root_index.parent().expect("workspace dir should exist");
    let old_path = root_dir.join("old.md");
    let new_path = root_dir.join("new.md");
    harness
        .send_file_moved(
            old_path.to_str().expect("old path should be utf-8"),
            new_path.to_str().expect("new path should be utf-8"),
        )
        .await;

    let first = harness
        .command("SyncPush", json!({}))
        .await
        .expect("SyncPush should return Some")
        .expect("SyncPush should succeed with error payload");
    assert!(
        first
            .get("errors")
            .and_then(|value| value.as_array())
            .is_some_and(|errors| !errors.is_empty()),
        "first sync should surface the transient remote delete failure: {first}"
    );

    let manifest_after_first = saved_manifest(&storage);
    assert_eq!(
        manifest_after_first.pending_deletes.len(),
        1,
        "failed remote delete should stay pending for retry"
    );
    assert_eq!(manifest_after_first.pending_deletes[0].path, "files/old.md",);

    let second = harness
        .command("SyncPush", json!({}))
        .await
        .expect("SyncPush should return Some")
        .expect("SyncPush should succeed");
    assert!(
        second
            .get("errors")
            .and_then(|value| value.as_array())
            .is_some_and(|errors| errors.is_empty()),
        "second sync should retry and clear the remote delete: {second}"
    );

    let manifest_after_second = saved_manifest(&storage);
    assert!(
        manifest_after_second.pending_deletes.is_empty(),
        "successful retry should clear pending deletes"
    );
}

#[tokio::test]
async fn sync_push_treats_remote_delete_404_as_acknowledged() {
    require_wasm!();
    let root_index = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - new.md\n---\n\n# Root\n",
        &[(
            "new.md",
            "---\ntitle: New\npart_of: index.md\n---\n\n# New\n",
        )],
    );
    let storage = Arc::new(RecordingStorage::new());
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.seed_object("ns-1", "files/old.md", b"# Old\n");
    namespace_provider.push_delete_error("files/old.md", "404 Not Found");
    let harness = load_with_storage_workspace_and_namespace(
        storage.clone(),
        &root_index,
        namespace_provider,
        None,
    );
    harness.init().await.expect("init");
    harness
        .command("set_config", json!({ "workspace_id": "ns-1" }))
        .await
        .expect("Some")
        .expect("set_config should succeed");

    let root_dir = root_index.parent().expect("workspace dir should exist");
    let old_path = root_dir.join("old.md");
    let new_path = root_dir.join("new.md");
    harness
        .send_file_moved(
            old_path.to_str().expect("old path should be utf-8"),
            new_path.to_str().expect("new path should be utf-8"),
        )
        .await;

    let result = harness
        .command("SyncPush", json!({}))
        .await
        .expect("SyncPush should return Some")
        .expect("SyncPush should succeed");
    assert!(
        result
            .get("errors")
            .and_then(|value| value.as_array())
            .is_some_and(|errors| errors.is_empty()),
        "404 delete should be treated as already removed: {result}"
    );

    let manifest = saved_manifest(&storage);
    assert!(
        manifest.pending_deletes.is_empty(),
        "404 delete should clear the tombstone"
    );
}

#[tokio::test]
async fn link_workspace_does_not_persist_workspace_id_when_initial_sync_fails() {
    require_wasm!();
    let root_index = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n\n# Root\n",
        &[(
            "note.md",
            "---\ntitle: Note\npart_of: index.md\n---\n\n# Note\n",
        )],
    );
    let storage = Arc::new(RecordingStorage::new());
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.fail_put_for_key("files/note.md", "simulated upload failure");
    let harness =
        load_with_storage_workspace_and_namespace(storage, &root_index, namespace_provider, None);
    harness.init().await.expect("init");

    let result = harness
        .command("LinkWorkspace", json!({ "remote_id": "ns-1" }))
        .await
        .expect("LinkWorkspace should return Some");
    assert!(
        result.is_err(),
        "initial sync failures should bubble up as a command error: {result:?}"
    );

    let root_contents =
        std::fs::read_to_string(&root_index).expect("root index should be readable");
    assert!(
        !root_contents.contains("workspace_id"),
        "workspace link should not be persisted after a failed initial sync:\n{root_contents}"
    );
}

#[tokio::test]
async fn upload_workspace_snapshot_fails_on_partial_uploads() {
    require_wasm!();
    let root_index = create_workspace(
        "index.md",
        "---\ntitle: Root\ncontents:\n  - note.md\n---\n\n# Root\n",
        &[(
            "note.md",
            "---\ntitle: Note\npart_of: index.md\n---\n\n# Note\n",
        )],
    );
    let storage = Arc::new(RecordingStorage::new());
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    namespace_provider.fail_put_for_key("files/note.md", "simulated upload failure");
    let harness =
        load_with_storage_workspace_and_namespace(storage, &root_index, namespace_provider, None);
    harness.init().await.expect("init");

    let result = harness
        .command("UploadWorkspaceSnapshot", json!({ "remote_id": "ns-1" }))
        .await
        .expect("UploadWorkspaceSnapshot should return Some");
    assert!(
        result.is_err(),
        "partial upload failures should be surfaced as command errors: {result:?}"
    );
}

#[tokio::test]
async fn ns_create_namespace_accepts_name_parameter() {
    require_wasm!();
    let namespace_provider = Arc::new(MockNamespaceProvider::new());
    let harness = PluginTestHarnessBuilder::new(WASM_PATH)
        .with_namespace_provider(namespace_provider)
        .build()
        .expect("Failed to load");
    harness.init().await.expect("init");

    let result = harness
        .command("NsCreateNamespace", json!({ "name": "Project Space" }))
        .await
        .expect("NsCreateNamespace should return Some")
        .expect("NsCreateNamespace should succeed");

    assert_eq!(
        result
            .get("metadata")
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str()),
        Some("Project Space")
    );
}
