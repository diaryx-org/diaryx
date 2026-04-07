//! Integration tests for the diaryx-sync Extism plugin.
//!
//! These tests load the pre-built WASM module via `PluginTestHarness` and
//! exercise the plugin's exports through the Extism runtime.
//!
//! Prerequisites: `cargo build --target wasm32-unknown-unknown --release`

use std::sync::Arc;

use diaryx_core::plugin::manifest::{PluginCapability, UiContribution};
use diaryx_extism::testing::*;
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
    let harness = load_with_storage(storage);
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
        "Provider should be ready with server_url + auth_token. Got: {result}"
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
