//! SyncPlugin — WorkspacePlugin that owns all CRDT state and handles sync commands.
//!
//! `SyncPlugin<FS>` is generic over the filesystem but type-erased at registration
//! via `Arc<dyn WorkspacePlugin>`. The plugin holds its own `FS` from construction,
//! so [`PluginContext`] does not need a filesystem handle.
//!
//! # Construction patterns
//!
//! ```ignore
//! // Fresh empty state
//! let plugin = SyncPlugin::new(fs, storage);
//!
//! // Load existing state from storage
//! let plugin = SyncPlugin::load(fs, storage)?;
//!
//! // Pre-configured CRDT instances (WASM backend with event callbacks)
//! let plugin = SyncPlugin::with_instances(fs, workspace_crdt, body_docs, storage);
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value as JsonValue;

use diaryx_core::error::Result as CoreResult;
use diaryx_core::export::Exporter;
use diaryx_core::frontmatter;
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::link_parser::{self, LinkFormat};
use diaryx_core::path_utils::normalize_sync_path;
use diaryx_core::plugin::{
    Plugin, PluginCapability, PluginContext, PluginError, PluginId, PluginManifest, SettingsField,
    UiContribution, WorkspaceOpenedEvent, WorkspacePlugin,
};
use diaryx_core::types::{BinaryRef, FileMetadata};

use crate::crdt_storage::{CrdtStorage, UpdateOrigin};
use diaryx_core::workspace::Workspace;

use crate::history::HistoryManager;
use crate::sync_handler::{GuestConfig, SyncHandler};
use crate::sync_manager::RustSyncManager;
use crate::{BodyDocManager, SyncMessage, WorkspaceCrdt};

// ============================================================================
// SyncPlugin struct
// ============================================================================

/// Plugin that owns all CRDT state and handles sync commands.
///
/// Generic over `FS` (filesystem), but erased to `Arc<dyn WorkspacePlugin>` at registration.
/// The `FS: Clone` bound is required for constructing `Workspace<FS>` and `Exporter<FS>`.
pub struct SyncPlugin<FS: AsyncFileSystem + Clone> {
    // CRDT components
    workspace_crdt: Arc<WorkspaceCrdt>,
    body_docs: Arc<BodyDocManager>,
    sync_handler: Arc<SyncHandler<FS>>,
    sync_manager: Arc<RustSyncManager<FS>>,
    storage: Arc<dyn CrdtStorage>,

    // Filesystem (for InitializeWorkspaceCrdt, audience filtering)
    fs: FS,

    // Runtime config (seeded from PluginContext, updated via events)
    workspace_root: RwLock<Option<PathBuf>>,
    link_format: RwLock<LinkFormat>,
}

// ============================================================================
// Constructors & Accessors
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    /// Create with fresh empty CRDT state.
    pub fn new(fs: FS, storage: Arc<dyn CrdtStorage>) -> Self {
        let workspace_crdt = Arc::new(WorkspaceCrdt::new(Arc::clone(&storage)));
        let body_docs = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let sync_handler = Arc::new(SyncHandler::new(fs.clone()));
        let sync_manager = Arc::new(RustSyncManager::new(
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_docs),
            Arc::clone(&sync_handler),
        ));
        Self {
            workspace_crdt,
            body_docs,
            sync_handler,
            sync_manager,
            storage,
            fs,
            workspace_root: RwLock::new(None),
            link_format: RwLock::new(LinkFormat::default()),
        }
    }

    /// Create by loading existing CRDT state from storage.
    pub fn load(fs: FS, storage: Arc<dyn CrdtStorage>) -> CoreResult<Self> {
        let workspace_crdt = Arc::new(WorkspaceCrdt::load(Arc::clone(&storage))?);
        let body_docs = Arc::new(BodyDocManager::new(Arc::clone(&storage)));
        let sync_handler = Arc::new(SyncHandler::new(fs.clone()));
        let sync_manager = Arc::new(RustSyncManager::new(
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_docs),
            Arc::clone(&sync_handler),
        ));
        Ok(Self {
            workspace_crdt,
            body_docs,
            sync_handler,
            sync_manager,
            storage,
            fs,
            workspace_root: RwLock::new(None),
            link_format: RwLock::new(LinkFormat::default()),
        })
    }

    /// Create with pre-configured CRDT instances.
    ///
    /// Use this when consumers need `Arc` handles for event callbacks before
    /// plugin registration (e.g., the WASM backend).
    pub fn with_instances(
        fs: FS,
        workspace_crdt: Arc<WorkspaceCrdt>,
        body_docs: Arc<BodyDocManager>,
        storage: Arc<dyn CrdtStorage>,
    ) -> Self {
        let sync_handler = Arc::new(SyncHandler::new(fs.clone()));
        let sync_manager = Arc::new(RustSyncManager::new(
            Arc::clone(&workspace_crdt),
            Arc::clone(&body_docs),
            Arc::clone(&sync_handler),
        ));
        Self {
            workspace_crdt,
            body_docs,
            sync_handler,
            sync_manager,
            storage,
            fs,
            workspace_root: RwLock::new(None),
            link_format: RwLock::new(LinkFormat::default()),
        }
    }

    /// Get the workspace CRDT handle.
    pub fn workspace_crdt(&self) -> Arc<WorkspaceCrdt> {
        Arc::clone(&self.workspace_crdt)
    }

    /// Get the body document manager handle.
    pub fn body_docs(&self) -> Arc<BodyDocManager> {
        Arc::clone(&self.body_docs)
    }

    /// Get the sync handler handle.
    pub fn sync_handler(&self) -> Arc<SyncHandler<FS>> {
        Arc::clone(&self.sync_handler)
    }

    /// Get the sync manager handle.
    pub fn sync_manager(&self) -> Arc<RustSyncManager<FS>> {
        Arc::clone(&self.sync_manager)
    }

    /// Get the storage handle.
    pub fn storage(&self) -> Arc<dyn CrdtStorage> {
        Arc::clone(&self.storage)
    }
}

// ============================================================================
// Manifest
// ============================================================================

/// Shared manifest for the Sync plugin (used by both native and WASM impls).
fn sync_plugin_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId("diaryx.sync".into()),
        name: "Sync".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        description: "Real-time CRDT sync across devices".into(),
        capabilities: vec![
            PluginCapability::WorkspaceEvents,
            PluginCapability::CrdtCommands,
            PluginCapability::SyncTransport,
        ],
        ui: vec![UiContribution::SettingsTab {
            id: "sync-settings".into(),
            label: "Sync".into(),
            icon: None,
            fields: vec![
                SettingsField::AuthStatus {
                    label: "Account".into(),
                    description: Some("Sign in to enable sync.".into()),
                },
                SettingsField::Conditional {
                    condition: "not_plus".into(),
                    fields: vec![
                        SettingsField::Section {
                            label: "Free Plan".into(),
                            description: Some(
                                "Free accounts can stay signed in on up to two devices and sync one hosted workspace."
                                    .into(),
                            ),
                        },
                        SettingsField::Section {
                            label: "Plus Unlocks".into(),
                            description: Some(
                                "Upgrade to Plus for up to ten synced workspaces and higher storage limits."
                                    .into(),
                            ),
                        },
                    ],
                },
                SettingsField::UpgradeBanner {
                    feature: "More Sync".into(),
                    description: Some(
                        "Free includes one synced workspace on up to two devices. Upgrade for more synced workspaces and storage."
                            .into(),
                    ),
                },
                SettingsField::Conditional {
                    condition: "plus".into(),
                    fields: vec![
                        SettingsField::Section {
                            label: "Connection".into(),
                            description: None,
                        },
                        SettingsField::Text {
                            key: "server_url".into(),
                            label: "Server URL".into(),
                            description: Some("Automatically configured when you sign in.".into()),
                            placeholder: Some("https://app.diaryx.org/api".into()),
                        },
                        SettingsField::Button {
                            label: "Check Status".into(),
                            command: "GetProviderStatus".into(),
                            variant: Some("outline".into()),
                        },
                    ],
                },
            ],
            component: None,
        }],
        cli: vec![],
    }
}

// ============================================================================
// Plugin + WorkspacePlugin trait implementations
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<FS: AsyncFileSystem + Clone + Send + Sync + 'static> Plugin for SyncPlugin<FS> {
    fn id(&self) -> PluginId {
        PluginId("diaryx.sync".into())
    }

    fn manifest(&self) -> PluginManifest {
        sync_plugin_manifest()
    }

    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(root) = &ctx.workspace_root {
            *self.workspace_root.write().unwrap() = Some(root.clone());
            self.sync_handler.set_workspace_root(root.clone());
        }
        *self.link_format.write().unwrap() = ctx.link_format;
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl<FS: AsyncFileSystem + Clone + 'static> Plugin for SyncPlugin<FS> {
    fn id(&self) -> PluginId {
        PluginId("diaryx.sync".into())
    }

    fn manifest(&self) -> PluginManifest {
        sync_plugin_manifest()
    }

    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        if let Some(root) = &ctx.workspace_root {
            *self.workspace_root.write().unwrap() = Some(root.clone());
            self.sync_handler.set_workspace_root(root.clone());
        }
        *self.link_format.write().unwrap() = ctx.link_format;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<FS: AsyncFileSystem + Clone + Send + Sync + 'static> WorkspacePlugin for SyncPlugin<FS> {
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        *self.workspace_root.write().unwrap() = Some(event.workspace_root.clone());
        self.sync_handler
            .set_workspace_root(event.workspace_root.clone());
    }

    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        Some(self.dispatch(cmd, params).await)
    }

    async fn notify_workspace_modified(&self) {
        if let Err(e) = self.sync_manager.emit_workspace_update() {
            log::warn!("SyncPlugin: failed to emit workspace update: {}", e);
        }
    }

    async fn on_body_doc_renamed(&self, old_path: &str, new_path: &str) {
        if let Err(e) = self.body_docs.rename(old_path, new_path) {
            log::warn!(
                "SyncPlugin: failed to rename body doc {} -> {}: {}",
                old_path,
                new_path,
                e
            );
        }
    }

    async fn on_body_doc_deleted(&self, path: &str) {
        if let Err(e) = self.body_docs.delete(path) {
            log::warn!("SyncPlugin: failed to delete body doc {}: {}", path, e);
        }
    }

    async fn track_file_for_sync(&self, canonical_path: &str) {
        if let Some(metadata) = self.workspace_crdt.get_file(canonical_path) {
            self.sync_manager.track_metadata(canonical_path, &metadata);
        }
    }

    fn get_canonical_path(&self, storage_path: &str) -> Option<String> {
        Some(self.sync_manager.get_canonical_path(storage_path))
    }

    fn track_content_for_sync(&self, canonical_path: &str, content: &str) {
        self.sync_manager.track_content(canonical_path, content);
    }

    fn get_file_title(&self, canonical_path: &str) -> Option<String> {
        self.workspace_crdt
            .get_file(canonical_path)
            .and_then(|m| m.title)
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
impl<FS: AsyncFileSystem + Clone + 'static> WorkspacePlugin for SyncPlugin<FS> {
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        *self.workspace_root.write().unwrap() = Some(event.workspace_root.clone());
        self.sync_handler
            .set_workspace_root(event.workspace_root.clone());
    }

    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        Some(self.dispatch(cmd, params).await)
    }

    async fn notify_workspace_modified(&self) {
        if let Err(e) = self.sync_manager.emit_workspace_update() {
            log::warn!("SyncPlugin: failed to emit workspace update: {}", e);
        }
    }

    async fn on_body_doc_renamed(&self, old_path: &str, new_path: &str) {
        if let Err(e) = self.body_docs.rename(old_path, new_path) {
            log::warn!(
                "SyncPlugin: failed to rename body doc {} -> {}: {}",
                old_path,
                new_path,
                e
            );
        }
    }

    async fn on_body_doc_deleted(&self, path: &str) {
        if let Err(e) = self.body_docs.delete(path) {
            log::warn!("SyncPlugin: failed to delete body doc {}: {}", path, e);
        }
    }

    async fn track_file_for_sync(&self, canonical_path: &str) {
        if let Some(metadata) = self.workspace_crdt.get_file(canonical_path) {
            self.sync_manager.track_metadata(canonical_path, &metadata);
        }
    }

    fn get_canonical_path(&self, storage_path: &str) -> Option<String> {
        Some(self.sync_manager.get_canonical_path(storage_path))
    }

    fn track_content_for_sync(&self, canonical_path: &str, content: &str) {
        self.sync_manager.track_content(canonical_path, content);
    }

    fn get_file_title(&self, canonical_path: &str) -> Option<String> {
        self.workspace_crdt
            .get_file(canonical_path)
            .and_then(|m| m.title)
    }
}

// ============================================================================
// Command dispatch
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    /// Route a command string to the appropriate handler method.
    async fn dispatch(&self, cmd: &str, params: JsonValue) -> Result<JsonValue, PluginError> {
        match cmd {
            // === Workspace CRDT State ===
            "GetSyncState" => self.cmd_get_sync_state(params),
            "GetFullState" => self.cmd_get_full_state(params),
            "ApplyRemoteUpdate" => self.cmd_apply_remote_update(params),
            "GetMissingUpdates" => self.cmd_get_missing_updates(params),
            "SaveCrdtState" => self.cmd_save_crdt_state(params),

            // === File Metadata ===
            "GetCrdtFile" => self.cmd_get_crdt_file(params),
            "SetCrdtFile" => self.cmd_set_crdt_file(params),
            "ListCrdtFiles" => self.cmd_list_crdt_files(params),

            // === Body Documents ===
            "GetBodyContent" => self.cmd_get_body_content(params),
            "SetBodyContent" => self.cmd_set_body_content(params),
            "ResetBodyDoc" => self.cmd_reset_body_doc(params),
            "GetBodySyncState" => self.cmd_get_body_sync_state(params),
            "GetBodyFullState" => self.cmd_get_body_full_state(params),
            "ApplyBodyUpdate" => self.cmd_apply_body_update(params),
            "GetBodyMissingUpdates" => self.cmd_get_body_missing_updates(params),
            "SaveBodyDoc" => self.cmd_save_body_doc(params),
            "SaveAllBodyDocs" => self.cmd_save_all_body_docs(),
            "ListLoadedBodyDocs" => self.cmd_list_loaded_body_docs(),
            "UnloadBodyDoc" => self.cmd_unload_body_doc(params),

            // === Y-Sync Protocol (CrdtOps-level) ===
            "CreateSyncStep1" => self.cmd_create_sync_step1(params),
            "HandleSyncMessage" => self.cmd_handle_sync_message(params).await,
            "CreateUpdateMessage" => self.cmd_create_update_message(params),

            // === Sync Handler ===
            "ConfigureSyncHandler" => self.cmd_configure_sync_handler(params),
            "GetStoragePath" => self.cmd_get_storage_path(params),
            "GetCanonicalPath" => self.cmd_get_canonical_path(params),
            "ApplyRemoteWorkspaceUpdateWithEffects" => {
                self.cmd_apply_remote_workspace_update_with_effects(params)
                    .await
            }
            "ApplyRemoteBodyUpdateWithEffects" => {
                self.cmd_apply_remote_body_update_with_effects(params).await
            }

            // === Sync Manager ===
            "HandleWorkspaceSyncMessage" => self.cmd_handle_workspace_sync_message(params).await,
            "HandleCrdtState" => self.cmd_handle_crdt_state(params).await,
            "CreateWorkspaceSyncStep1" => self.cmd_create_workspace_sync_step1(),
            "CreateWorkspaceUpdate" => self.cmd_create_workspace_update(params),
            "InitBodySync" => self.cmd_init_body_sync(params),
            "CloseBodySync" => self.cmd_close_body_sync(params),
            "HandleBodySyncMessage" => self.cmd_handle_body_sync_message(params).await,
            "CreateBodySyncStep1" => self.cmd_create_body_sync_step1(params),
            "CreateBodyUpdate" => self.cmd_create_body_update(params),
            "IsSyncComplete" => self.cmd_is_sync_complete(),
            "IsWorkspaceSynced" => self.cmd_is_workspace_synced(),
            "IsBodySynced" => self.cmd_is_body_synced(params),
            "MarkSyncComplete" => self.cmd_mark_sync_complete(),
            "GetActiveSyncs" => self.cmd_get_active_syncs(),
            "TrackContent" => self.cmd_track_content(params),
            "IsEcho" => self.cmd_is_echo(params),
            "ClearTrackedContent" => self.cmd_clear_tracked_content(params),
            "ResetSyncState" => self.cmd_reset_sync_state(),
            "TriggerWorkspaceSync" => self.cmd_trigger_workspace_sync(),

            // === History ===
            "GetHistory" => self.cmd_get_history(params),
            "GetFileHistory" => self.cmd_get_file_history(params),
            "RestoreVersion" => self.cmd_restore_version(params),
            "GetVersionDiff" => self.cmd_get_version_diff(params),
            "GetStateAt" => self.cmd_get_state_at(params),

            // === Workspace Initialization ===
            "InitializeWorkspaceCrdt" => self.cmd_initialize_workspace_crdt(params).await,

            // === Materialization ===
            "MaterializeWorkspace" => self.cmd_materialize_workspace(),

            // === Sync Status / Provider / Share ===
            "GetSyncStatus" => self.cmd_get_sync_status(params),
            "GetProviderStatus" => self.cmd_get_provider_status(params),
            "ListRemoteWorkspaces" => self.cmd_list_remote_workspaces(params),
            "LinkWorkspace" => self.cmd_link_workspace(params),
            "UnlinkWorkspace" => self.cmd_unlink_workspace(params),
            "DownloadWorkspace" => self.cmd_download_workspace(params),
            "CreateShareSession" => self.cmd_create_share_session(params),
            "JoinShareSession" => self.cmd_join_share_session(params),
            "EndShareSession" => self.cmd_end_share_session(params),
            "SetShareReadOnly" => self.cmd_set_share_read_only(params),

            other => Err(PluginError::CommandError(format!(
                "Unknown sync command: {other}"
            ))),
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn decode_b64(val: &JsonValue) -> Result<Vec<u8>, PluginError> {
    match val {
        JsonValue::String(s) => BASE64
            .decode(s)
            .map_err(|e| PluginError::CommandError(format!("Invalid base64: {e}"))),
        JsonValue::Array(arr) => {
            // Accept array of numbers (byte array)
            arr.iter()
                .map(|v| {
                    v.as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or_else(|| {
                            PluginError::CommandError("Invalid byte in array".to_string())
                        })
                })
                .collect()
        }
        _ => Err(PluginError::CommandError(
            "Expected base64 string or byte array".to_string(),
        )),
    }
}

fn encode_b64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

fn get_str(params: &JsonValue, key: &str) -> Result<String, PluginError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| PluginError::CommandError(format!("Missing parameter: {key}")))
}

fn get_bytes(params: &JsonValue, key: &str) -> Result<Vec<u8>, PluginError> {
    params
        .get(key)
        .ok_or_else(|| PluginError::CommandError(format!("Missing parameter: {key}")))
        .and_then(decode_b64)
}

fn get_bool(params: &JsonValue, key: &str) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn get_opt_i64(params: &JsonValue, key: &str) -> Option<i64> {
    params.get(key).and_then(|v| v.as_i64())
}

fn map_err(e: impl std::fmt::Display) -> PluginError {
    PluginError::CommandError(e.to_string())
}

/// Check if a doc name refers to the workspace CRDT.
fn is_workspace_doc(doc_name: &str) -> bool {
    doc_name == "workspace" || doc_name.ends_with(":workspace")
}

/// Merge attachment refs from existing CRDT entry into incoming metadata.
fn merge_attachment_refs(existing: &FileMetadata, incoming: &mut FileMetadata) {
    if existing.attachments.is_empty() {
        return;
    }
    if incoming.attachments.is_empty() {
        incoming.attachments = existing.attachments.clone();
        return;
    }
    for attachment in &mut incoming.attachments {
        if attachment.hash.is_empty() {
            if let Some(existing_ref) = existing
                .attachments
                .iter()
                .find(|r| r.path == attachment.path && !r.hash.is_empty())
            {
                attachment.hash = existing_ref.hash.clone();
                attachment.mime_type = existing_ref.mime_type.clone();
                attachment.size = existing_ref.size;
                attachment.uploaded_at = existing_ref.uploaded_at;
                attachment.source = existing_ref.source.clone();
            }
        }
    }
}

// ============================================================================
// Command handlers — Workspace CRDT State
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_get_sync_state(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        let sv = self.workspace_crdt.encode_state_vector();
        Ok(serde_json::json!({ "data": encode_b64(&sv) }))
    }

    fn cmd_get_full_state(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        let state = self.workspace_crdt.encode_state_as_update();
        Ok(serde_json::json!({ "data": encode_b64(&state) }))
    }

    fn cmd_apply_remote_update(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let update = get_bytes(&params, "update")?;
        let update_id = self
            .workspace_crdt
            .apply_update(&update, UpdateOrigin::Remote)
            .map_err(map_err)?;
        Ok(serde_json::json!({ "update_id": update_id }))
    }

    fn cmd_get_missing_updates(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let remote_sv = get_bytes(&params, "remote_state_vector")?;
        let diff = self
            .workspace_crdt
            .encode_diff(&remote_sv)
            .map_err(map_err)?;
        Ok(serde_json::json!({ "data": encode_b64(&diff) }))
    }

    fn cmd_save_crdt_state(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        self.workspace_crdt.save().map_err(map_err)?;
        Ok(JsonValue::Null)
    }
}

// ============================================================================
// Command handlers — Sync Status / Provider / Share
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_get_sync_status(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Ok(serde_json::json!({
            "state": "idle",
            "label": "Idle",
            "detail": JsonValue::Null,
            "progress": JsonValue::Null
        }))
    }

    fn host_runtime_only(command: &str) -> PluginError {
        PluginError::CommandError(format!(
            "{command} is only available in host-integrated runtimes"
        ))
    }

    fn cmd_get_provider_status(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("GetProviderStatus"))
    }

    fn cmd_list_remote_workspaces(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("ListRemoteWorkspaces"))
    }

    fn cmd_link_workspace(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("LinkWorkspace"))
    }

    fn cmd_unlink_workspace(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("UnlinkWorkspace"))
    }

    fn cmd_download_workspace(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("DownloadWorkspace"))
    }

    fn cmd_create_share_session(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("CreateShareSession"))
    }

    fn cmd_join_share_session(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("JoinShareSession"))
    }

    fn cmd_end_share_session(&self, _params: JsonValue) -> Result<JsonValue, PluginError> {
        Err(Self::host_runtime_only("EndShareSession"))
    }

    fn cmd_set_share_read_only(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let read_only = get_bool(&params, "read_only");
        Ok(serde_json::json!({ "read_only": read_only }))
    }
}

// ============================================================================
// Command handlers — File Metadata
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_get_crdt_file(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let path = get_str(&params, "path")?;
        let file = self.workspace_crdt.get_file(&path);
        Ok(serde_json::to_value(file).map_err(map_err)?)
    }

    fn cmd_set_crdt_file(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let path = get_str(&params, "path")?;
        let metadata_val = params
            .get("metadata")
            .ok_or_else(|| PluginError::CommandError("Missing parameter: metadata".into()))?;
        let mut metadata: FileMetadata =
            serde_json::from_value(metadata_val.clone()).map_err(map_err)?;

        if let Some(existing) = self.workspace_crdt.get_file(&path) {
            merge_attachment_refs(&existing, &mut metadata);
        }

        self.workspace_crdt
            .set_file(&path, metadata)
            .map_err(map_err)?;

        // Emit workspace sync
        if let Err(e) = self.sync_manager.emit_workspace_update() {
            log::warn!("Failed to emit workspace sync for SetCrdtFile: {}", e);
        }

        Ok(JsonValue::Null)
    }

    fn cmd_list_crdt_files(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let include_deleted = get_bool(&params, "include_deleted");
        let files = if include_deleted {
            self.workspace_crdt.list_files()
        } else {
            self.workspace_crdt.list_active_files()
        };
        Ok(serde_json::to_value(files).map_err(map_err)?)
    }
}

// ============================================================================
// Command handlers — Body Documents
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_get_body_content(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let content = self
            .body_docs
            .get(&doc_name)
            .map(|doc| doc.get_body())
            .unwrap_or_default();
        Ok(serde_json::json!({ "content": content }))
    }

    fn cmd_set_body_content(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let content = get_str(&params, "content")?;
        let doc = self.body_docs.get_or_create(&doc_name);
        doc.set_body(&content).map_err(map_err)?;
        Ok(JsonValue::Null)
    }

    fn cmd_reset_body_doc(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        self.body_docs.create(&doc_name);
        Ok(JsonValue::Null)
    }

    fn cmd_get_body_sync_state(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let state = self.body_docs.get_sync_state(&doc_name).unwrap_or_default();
        Ok(serde_json::json!({ "data": encode_b64(&state) }))
    }

    fn cmd_get_body_full_state(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let state = self.body_docs.get_full_state(&doc_name).unwrap_or_default();
        Ok(serde_json::json!({ "data": encode_b64(&state) }))
    }

    fn cmd_apply_body_update(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let update = get_bytes(&params, "update")?;
        let update_id = self
            .body_docs
            .apply_update(&doc_name, &update, UpdateOrigin::Remote)
            .map_err(map_err)?;
        Ok(serde_json::json!({ "update_id": update_id }))
    }

    fn cmd_get_body_missing_updates(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let remote_sv = get_bytes(&params, "remote_state_vector")?;
        let diff = self
            .body_docs
            .get_diff(&doc_name, &remote_sv)
            .map_err(map_err)?;
        Ok(serde_json::json!({ "data": encode_b64(&diff) }))
    }

    fn cmd_save_body_doc(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        self.body_docs.save(&doc_name).map_err(map_err)?;
        Ok(JsonValue::Null)
    }

    fn cmd_save_all_body_docs(&self) -> Result<JsonValue, PluginError> {
        self.body_docs.save_all().map_err(map_err)?;
        Ok(JsonValue::Null)
    }

    fn cmd_list_loaded_body_docs(&self) -> Result<JsonValue, PluginError> {
        let docs = self.body_docs.loaded_docs();
        Ok(serde_json::to_value(docs).map_err(map_err)?)
    }

    fn cmd_unload_body_doc(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        self.body_docs.unload(&doc_name);
        Ok(JsonValue::Null)
    }
}

// ============================================================================
// Command handlers — Y-Sync Protocol (CrdtOps-level)
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_create_sync_step1(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let message = if is_workspace_doc(&doc_name) {
            let sv = self.workspace_crdt.encode_state_vector();
            SyncMessage::SyncStep1(sv).encode()
        } else {
            let doc = self.body_docs.get_or_create(&doc_name);
            let sv = doc.encode_state_vector();
            SyncMessage::SyncStep1(sv).encode()
        };
        Ok(serde_json::json!({ "data": encode_b64(&message) }))
    }

    async fn cmd_handle_sync_message(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let message = get_bytes(&params, "message")?;
        let write_to_disk = get_bool(&params, "write_to_disk");

        // Decode all sub-messages
        let messages = SyncMessage::decode_all(&message).map_err(map_err)?;
        if messages.is_empty() {
            return Ok(JsonValue::Null);
        }

        let mut response: Option<Vec<u8>> = None;
        let mut all_changed_files = Vec::new();

        for sync_msg in messages {
            if is_workspace_doc(&doc_name) {
                let (msg_response, changed_files) =
                    self.handle_workspace_sync_msg_with_changes(sync_msg)?;
                all_changed_files.extend(changed_files);
                if let Some(resp) = msg_response {
                    match response.as_mut() {
                        Some(existing) => existing.extend_from_slice(&resp),
                        None => response = Some(resp),
                    }
                }
            } else {
                let msg_response = self.handle_body_sync_msg(&doc_name, sync_msg)?;
                if let Some(resp) = msg_response {
                    match response.as_mut() {
                        Some(existing) => existing.extend_from_slice(&resp),
                        None => response = Some(resp),
                    }
                }
            }
        }

        // Write changed files to disk if requested
        if write_to_disk && !all_changed_files.is_empty() {
            let files_to_sync: Vec<(String, FileMetadata)> = all_changed_files
                .iter()
                .filter_map(|path| {
                    self.workspace_crdt
                        .get_file(path)
                        .map(|m| (path.clone(), m))
                })
                .collect();
            if !files_to_sync.is_empty() {
                self.sync_handler
                    .handle_remote_metadata_update(
                        files_to_sync,
                        Vec::new(),
                        Some(self.body_docs.as_ref()),
                        true,
                    )
                    .await
                    .map_err(map_err)?;
            }
        }

        match response {
            Some(data) => Ok(serde_json::json!({ "data": encode_b64(&data) })),
            None => Ok(JsonValue::Null),
        }
    }

    fn cmd_create_update_message(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let update = get_bytes(&params, "update")?;
        let message = SyncMessage::Update(update).encode();
        Ok(serde_json::json!({ "data": encode_b64(&message) }))
    }

    // ---- Internal sync message handlers (mirrors CrdtOps) ----

    fn handle_workspace_sync_msg_with_changes(
        &self,
        msg: SyncMessage,
    ) -> Result<(Option<Vec<u8>>, Vec<String>), PluginError> {
        match msg {
            SyncMessage::SyncStep1(remote_sv) => {
                let diff = self
                    .workspace_crdt
                    .encode_diff(&remote_sv)
                    .map_err(map_err)?;
                let step2 = SyncMessage::SyncStep2(diff).encode();
                let our_sv = self.workspace_crdt.encode_state_vector();
                let step1 = SyncMessage::SyncStep1(our_sv).encode();
                let mut combined = step2;
                combined.extend_from_slice(&step1);
                Ok((Some(combined), Vec::new()))
            }
            SyncMessage::SyncStep2(update) => {
                if update.is_empty() {
                    return Ok((None, Vec::new()));
                }
                let (_, files, _renames) = self
                    .workspace_crdt
                    .apply_update_tracking_changes(&update, UpdateOrigin::Sync)
                    .map_err(map_err)?;
                Ok((None, files))
            }
            SyncMessage::Update(update) => {
                if update.is_empty() {
                    return Ok((None, Vec::new()));
                }
                let (_, files, _renames) = self
                    .workspace_crdt
                    .apply_update_tracking_changes(&update, UpdateOrigin::Remote)
                    .map_err(map_err)?;
                Ok((None, files))
            }
        }
    }

    fn handle_body_sync_msg(
        &self,
        doc_name: &str,
        msg: SyncMessage,
    ) -> Result<Option<Vec<u8>>, PluginError> {
        let doc = self.body_docs.get_or_create(doc_name);
        match msg {
            SyncMessage::SyncStep1(remote_sv) => {
                let diff = doc.encode_diff(&remote_sv).map_err(map_err)?;
                let step2 = SyncMessage::SyncStep2(diff).encode();
                let our_sv = doc.encode_state_vector();
                let step1 = SyncMessage::SyncStep1(our_sv).encode();
                let mut combined = step2;
                combined.extend_from_slice(&step1);
                Ok(Some(combined))
            }
            SyncMessage::SyncStep2(update) => {
                if !update.is_empty() {
                    doc.apply_update(&update, UpdateOrigin::Sync)
                        .map_err(map_err)?;
                }
                Ok(None)
            }
            SyncMessage::Update(update) => {
                if !update.is_empty() {
                    doc.apply_update(&update, UpdateOrigin::Remote)
                        .map_err(map_err)?;
                }
                Ok(None)
            }
        }
    }
}

// ============================================================================
// Command handlers — Sync Handler
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_configure_sync_handler(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let guest_join_code = params
            .get("guest_join_code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let uses_opfs = get_bool(&params, "uses_opfs");

        let config = guest_join_code.map(|join_code| GuestConfig {
            join_code,
            uses_opfs,
        });
        self.sync_handler.configure_guest(config);
        Ok(JsonValue::Null)
    }

    fn cmd_get_storage_path(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let canonical_path = get_str(&params, "canonical_path")?;
        let storage_path = self.sync_handler.get_storage_path(&canonical_path);
        Ok(serde_json::json!({ "path": storage_path.to_string_lossy() }))
    }

    fn cmd_get_canonical_path(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let storage_path = get_str(&params, "storage_path")?;
        let canonical = self.sync_handler.get_canonical_path(&storage_path);
        Ok(serde_json::json!({ "path": canonical }))
    }

    async fn cmd_apply_remote_workspace_update_with_effects(
        &self,
        params: JsonValue,
    ) -> Result<JsonValue, PluginError> {
        let update = get_bytes(&params, "update")?;
        let write_to_disk = get_bool(&params, "write_to_disk");

        let (update_id, changed_paths, renames) = self
            .workspace_crdt
            .apply_update_tracking_changes(&update, UpdateOrigin::Remote)
            .map_err(map_err)?;

        if write_to_disk {
            let files: Vec<(String, FileMetadata)> = changed_paths
                .iter()
                .filter_map(|path| {
                    self.workspace_crdt
                        .get_file(path)
                        .map(|m| (path.clone(), m))
                })
                .collect();
            self.sync_handler
                .handle_remote_metadata_update(files, renames, Some(self.body_docs.as_ref()), true)
                .await
                .map_err(map_err)?;
        }

        Ok(serde_json::json!({ "update_id": update_id }))
    }

    async fn cmd_apply_remote_body_update_with_effects(
        &self,
        params: JsonValue,
    ) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let update = get_bytes(&params, "update")?;
        let write_to_disk = get_bool(&params, "write_to_disk");

        let doc = self.body_docs.get_or_create(&doc_name);
        let update_id = doc
            .apply_update(&update, UpdateOrigin::Remote)
            .map_err(map_err)?;

        if write_to_disk {
            let body = doc.get_body();
            let metadata = self.workspace_crdt.get_file(&doc_name);
            self.sync_handler
                .handle_remote_body_update(&doc_name, &body, metadata.as_ref())
                .await
                .map_err(map_err)?;
        }

        Ok(serde_json::json!({ "update_id": update_id }))
    }
}

// ============================================================================
// Command handlers — Sync Manager
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    async fn cmd_handle_workspace_sync_message(
        &self,
        params: JsonValue,
    ) -> Result<JsonValue, PluginError> {
        let message = get_bytes(&params, "message")?;
        let write_to_disk = get_bool(&params, "write_to_disk");

        let result = self
            .sync_manager
            .handle_workspace_message(&message, write_to_disk)
            .await
            .map_err(map_err)?;

        Ok(serde_json::json!({
            "response": result.response.map(|r| encode_b64(&r)),
            "changed_files": result.changed_files,
            "sync_complete": result.sync_complete,
        }))
    }

    async fn cmd_handle_crdt_state(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let state = get_bytes(&params, "state")?;
        let file_count = self
            .sync_manager
            .handle_crdt_state(&state)
            .await
            .map_err(map_err)?;
        log::info!(
            "[SyncPlugin] HandleCrdtState: applied state, {} files in workspace",
            file_count
        );
        Ok(JsonValue::Null)
    }

    fn cmd_create_workspace_sync_step1(&self) -> Result<JsonValue, PluginError> {
        let step1 = self.sync_manager.create_workspace_sync_step1();
        Ok(serde_json::json!({ "data": encode_b64(&step1) }))
    }

    fn cmd_create_workspace_update(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let since_sv = params
            .get("since_state_vector")
            .and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    Some(decode_b64(v))
                }
            })
            .transpose()?;

        let update = self
            .sync_manager
            .create_workspace_update(since_sv.as_deref())
            .map_err(map_err)?;
        Ok(serde_json::json!({ "data": encode_b64(&update) }))
    }

    fn cmd_init_body_sync(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        self.sync_manager.init_body_sync(&doc_name);
        Ok(JsonValue::Null)
    }

    fn cmd_close_body_sync(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        self.sync_manager.close_body_sync(&doc_name);
        Ok(JsonValue::Null)
    }

    async fn cmd_handle_body_sync_message(
        &self,
        params: JsonValue,
    ) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let message = get_bytes(&params, "message")?;
        let write_to_disk = get_bool(&params, "write_to_disk");

        let result = self
            .sync_manager
            .handle_body_message(&doc_name, &message, write_to_disk)
            .await
            .map_err(map_err)?;

        Ok(serde_json::json!({
            "response": result.response.map(|r| encode_b64(&r)),
            "content": result.content,
            "is_echo": result.is_echo,
        }))
    }

    fn cmd_create_body_sync_step1(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let step1 = self.sync_manager.create_body_sync_step1(&doc_name);
        Ok(serde_json::json!({ "data": encode_b64(&step1) }))
    }

    fn cmd_create_body_update(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let content = get_str(&params, "content")?;
        let update = self
            .sync_manager
            .create_body_update(&doc_name, &content)
            .map_err(map_err)?;
        Ok(serde_json::json!({ "data": encode_b64(&update) }))
    }

    fn cmd_is_sync_complete(&self) -> Result<JsonValue, PluginError> {
        Ok(serde_json::json!({ "complete": self.sync_manager.is_sync_complete() }))
    }

    fn cmd_is_workspace_synced(&self) -> Result<JsonValue, PluginError> {
        Ok(serde_json::json!({ "synced": self.sync_manager.is_workspace_synced() }))
    }

    fn cmd_is_body_synced(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        Ok(serde_json::json!({ "synced": self.sync_manager.is_body_synced(&doc_name) }))
    }

    fn cmd_mark_sync_complete(&self) -> Result<JsonValue, PluginError> {
        self.sync_manager.mark_sync_complete();
        Ok(JsonValue::Null)
    }

    fn cmd_get_active_syncs(&self) -> Result<JsonValue, PluginError> {
        let syncs = self.sync_manager.get_active_syncs();
        Ok(serde_json::to_value(syncs).map_err(map_err)?)
    }

    fn cmd_track_content(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let path = get_str(&params, "path")?;
        let content = get_str(&params, "content")?;
        self.sync_manager.track_content(&path, &content);
        Ok(JsonValue::Null)
    }

    fn cmd_is_echo(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let path = get_str(&params, "path")?;
        let content = get_str(&params, "content")?;
        Ok(serde_json::json!({ "is_echo": self.sync_manager.is_echo(&path, &content) }))
    }

    fn cmd_clear_tracked_content(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let path = get_str(&params, "path")?;
        self.sync_manager.clear_tracked_content(&path);
        Ok(JsonValue::Null)
    }

    fn cmd_reset_sync_state(&self) -> Result<JsonValue, PluginError> {
        self.sync_manager.reset();
        Ok(JsonValue::Null)
    }

    fn cmd_trigger_workspace_sync(&self) -> Result<JsonValue, PluginError> {
        let update = self
            .sync_manager
            .create_workspace_update(None)
            .map_err(map_err)?;
        if update.is_empty() {
            Ok(JsonValue::Null)
        } else {
            Ok(serde_json::json!({ "data": encode_b64(&update) }))
        }
    }
}

// ============================================================================
// Command handlers — History
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    fn cmd_get_history(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let limit = get_opt_i64(&params, "limit").map(|n| n as usize);
        let history_manager = HistoryManager::new(Arc::clone(&self.storage));
        let history = history_manager
            .get_history(&doc_name, limit)
            .map_err(map_err)?;
        Ok(serde_json::to_value(history).map_err(map_err)?)
    }

    fn cmd_get_file_history(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let file_path = get_str(&params, "file_path")?;
        let limit = get_opt_i64(&params, "limit").map(|n| n as usize);
        let history_manager = HistoryManager::new(Arc::clone(&self.storage));
        let history = history_manager
            .get_file_history(&file_path, limit)
            .map_err(map_err)?;
        Ok(serde_json::to_value(history).map_err(map_err)?)
    }

    fn cmd_restore_version(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let update_id = get_opt_i64(&params, "update_id")
            .ok_or_else(|| PluginError::CommandError("Missing parameter: update_id".into()))?;
        let history_manager = HistoryManager::new(Arc::clone(&self.storage));
        let restore_update = history_manager
            .create_restore_update(&doc_name, update_id)
            .map_err(map_err)?;
        self.workspace_crdt
            .apply_update(&restore_update, UpdateOrigin::Local)
            .map_err(map_err)?;
        self.workspace_crdt.save().map_err(map_err)?;
        Ok(JsonValue::Null)
    }

    fn cmd_get_version_diff(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let from_id = get_opt_i64(&params, "from_id")
            .ok_or_else(|| PluginError::CommandError("Missing parameter: from_id".into()))?;
        let to_id = get_opt_i64(&params, "to_id")
            .ok_or_else(|| PluginError::CommandError("Missing parameter: to_id".into()))?;
        let history_manager = HistoryManager::new(Arc::clone(&self.storage));
        let diffs = history_manager
            .diff(&doc_name, from_id, to_id)
            .map_err(map_err)?;
        Ok(serde_json::to_value(diffs).map_err(map_err)?)
    }

    fn cmd_get_state_at(&self, params: JsonValue) -> Result<JsonValue, PluginError> {
        let doc_name = get_str(&params, "doc_name")?;
        let update_id = get_opt_i64(&params, "update_id")
            .ok_or_else(|| PluginError::CommandError("Missing parameter: update_id".into()))?;
        let history_manager = HistoryManager::new(Arc::clone(&self.storage));
        let state = history_manager
            .get_state_at(&doc_name, update_id)
            .map_err(map_err)?;
        match state {
            Some(data) => Ok(serde_json::json!({ "data": encode_b64(&data) })),
            None => Ok(JsonValue::Null),
        }
    }
}

// ============================================================================
// Command handler — MaterializeWorkspace
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    /// Materialize all active (non-deleted) files from the CRDT into a JSON array
    /// of `{ path, content }` objects. Used by the CLI for git commit operations.
    fn cmd_materialize_workspace(&self) -> Result<JsonValue, PluginError> {
        let workspace_id = self
            .workspace_crdt
            .doc_name()
            .strip_prefix("workspace:")
            .unwrap_or("default");

        let result = crate::materialize::materialize_workspace(
            &self.workspace_crdt,
            &self.body_docs,
            workspace_id,
        );

        let files: Vec<JsonValue> = result
            .files
            .into_iter()
            .map(|f| {
                serde_json::json!({
                    "path": f.path,
                    "content": f.content,
                })
            })
            .collect();

        Ok(serde_json::json!({ "files": files }))
    }
}

// ============================================================================
// Command handler — InitializeWorkspaceCrdt
// ============================================================================

impl<FS: AsyncFileSystem + Clone + 'static> SyncPlugin<FS> {
    async fn cmd_initialize_workspace_crdt(
        &self,
        params: JsonValue,
    ) -> Result<JsonValue, PluginError> {
        let workspace_path = get_str(&params, "workspace_path")?;
        let audience = params
            .get("audience")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Log initial CRDT state for debugging
        let initial_files: Vec<_> = self.workspace_crdt.list_files().into_iter().collect();
        log::debug!(
            "[InitializeWorkspaceCrdt] INITIAL CRDT state: {} files: {:?}",
            initial_files.len(),
            initial_files.iter().take(10).collect::<Vec<_>>()
        );

        // Construct a Workspace from our FS
        let link_fmt = *self.link_format.read().unwrap();
        let root_path = PathBuf::from(&workspace_path);

        // base_path is the workspace root directory
        let base_path = if root_path.extension().is_some_and(|ext| ext == "md") {
            root_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| root_path.clone())
        } else {
            root_path.clone()
        };
        log::debug!(
            "[InitializeWorkspaceCrdt] workspace_path={:?}, base_path={:?}",
            workspace_path,
            base_path
        );

        let ws = Workspace::with_link_format(self.fs.clone(), base_path.clone(), link_fmt);

        // Find root index file
        let root_index = if root_path.extension().is_some_and(|ext| ext == "md") {
            root_path.clone()
        } else {
            ws.find_root_index_in_dir(&root_path)
                .await
                .map_err(map_err)?
                .ok_or_else(|| {
                    PluginError::CommandError(format!(
                        "No workspace root index found in {:?}",
                        root_path
                    ))
                })?
        };

        // Get workspace config for link format and default_audience
        let ws_config = ws.get_workspace_config(&root_index).await.ok();
        let link_format_hint = ws_config.as_ref().map(|cfg| cfg.link_format);
        let default_audience = ws_config
            .as_ref()
            .and_then(|cfg| cfg.default_audience.clone());

        // Audience filtering
        let allowed_paths: Option<HashSet<PathBuf>> = if let Some(ref aud) = audience {
            let exporter = Exporter::new(self.fs.clone());
            let plan = exporter
                .plan_export(
                    &root_index,
                    aud,
                    Path::new("/tmp"),
                    default_audience.as_deref(),
                )
                .await
                .map_err(map_err)?;
            Some(
                plan.included
                    .iter()
                    .map(|f| f.source_path.clone())
                    .collect(),
            )
        } else {
            None
        };

        // Build tree
        log::debug!(
            "[InitializeWorkspaceCrdt] Building tree from root_index={:?}",
            root_index
        );
        let tree = ws
            .build_tree_with_depth(&root_index, None, &mut HashSet::new())
            .await
            .map_err(map_err)?;
        log::debug!(
            "[InitializeWorkspaceCrdt] Tree built: root={:?}, children={}",
            tree.path,
            tree.children.len()
        );

        // Collect all files with their metadata using iterative tree walk
        let mut files_to_add: Vec<(String, FileMetadata)> = Vec::new();
        let mut stack: Vec<(&diaryx_core::workspace::TreeNode, Option<String>)> =
            vec![(&tree, None)];
        let mut files_updated_from_disk: Vec<String> = Vec::new();

        while let Some((node, parent_path)) = stack.pop() {
            let absolute_path = node.path.to_string_lossy().to_string();

            // Convert absolute path to workspace-relative canonical path
            let canonical_path = if base_path.as_os_str() == "." {
                normalize_sync_path(&node.path.to_string_lossy())
            } else {
                node.path
                    .strip_prefix(&base_path)
                    .map(|p| normalize_sync_path(&p.to_string_lossy()))
                    .unwrap_or_else(|_| {
                        log::warn!(
                            "[InitializeWorkspaceCrdt] Failed to strip prefix {:?} from {:?}",
                            base_path,
                            node.path
                        );
                        normalize_sync_path(&absolute_path)
                    })
            };

            // Skip files not in allowed set (audience filtering)
            if let Some(ref allowed) = allowed_paths {
                if !allowed.contains(&node.path) {
                    continue;
                }
            }

            // Get file modification time
            let file_mtime = self.fs.get_modified_time(&node.path).await;

            // Reconciliation: check existing CRDT entry
            let existing_crdt_entry = self.workspace_crdt.get_file(&canonical_path);

            if let Some(crdt_entry) = &existing_crdt_entry {
                if crdt_entry.deleted {
                    // Trust the tombstone — remove stale disk copy
                    log::info!(
                        "[InitializeWorkspaceCrdt] Skipping deleted file {} (CRDT tombstone)",
                        canonical_path
                    );
                    if let Err(e) = self.fs.delete_file(&node.path).await {
                        log::warn!(
                            "[InitializeWorkspaceCrdt] Failed to clean up stale file {}: {:?}",
                            canonical_path,
                            e
                        );
                    }
                    continue;
                }

                let should_keep_crdt = match file_mtime {
                    Some(fmtime) => crdt_entry.modified_at >= fmtime,
                    None => true,
                };

                if should_keep_crdt {
                    for child in node.children.iter().rev() {
                        stack.push((child, Some(canonical_path.clone())));
                    }
                    continue;
                }
            }

            // File is newer or no CRDT entry — read and update
            let content = match self.fs.read_to_string(Path::new(&absolute_path)).await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!(
                        "[InitializeWorkspaceCrdt] Could not read {}: {:?}",
                        absolute_path,
                        e
                    );
                    continue;
                }
            };

            let parsed = match frontmatter::parse_or_empty(&content) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!(
                        "[InitializeWorkspaceCrdt] Parse error for {}: {:?}",
                        canonical_path,
                        e
                    );
                    continue;
                }
            };

            if existing_crdt_entry.is_some() {
                files_updated_from_disk.push(canonical_path.clone());
                log::info!(
                    "[InitializeWorkspaceCrdt] Updating {} from disk (file is newer)",
                    canonical_path
                );
            }

            // Build FileMetadata
            let title = parsed
                .frontmatter
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let contents: Option<Vec<String>> = parsed
                .frontmatter
                .get("contents")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str())
                        .map(|raw_value| {
                            let parsed_link = link_parser::parse_link(raw_value);
                            link_parser::to_canonical_with_link_format(
                                &parsed_link,
                                Path::new(&canonical_path),
                                link_format_hint,
                            )
                        })
                        .collect()
                });

            let file_audience: Option<Vec<String>> = parsed
                .frontmatter
                .get("audience")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                });

            let description = parsed
                .frontmatter
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let attachments_note_paths: Vec<String> = parsed
                .frontmatter
                .get("attachments")
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str())
                        .map(|raw_value| {
                            let parsed_link = link_parser::parse_link(raw_value);
                            link_parser::to_canonical_with_link_format(
                                &parsed_link,
                                Path::new(&canonical_path),
                                link_format_hint,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();

            // Resolve attachment notes to their actual binary paths
            let mut attachments_list: Vec<String> =
                Vec::with_capacity(attachments_note_paths.len());
            for note_path in attachments_note_paths {
                let note_full = base_path.join(&note_path);
                if let Some(binary_path) = ws
                    .resolve_attachment_binary(&note_full, &base_path, link_format_hint)
                    .await
                {
                    attachments_list.push(binary_path);
                } else {
                    // Not an attachment note, or no `attachment` field — use as-is
                    attachments_list.push(note_path);
                }
            }

            let attachments: Vec<BinaryRef> = attachments_list
                .into_iter()
                .map(|path| {
                    let existing_ref = existing_crdt_entry.as_ref().and_then(|entry| {
                        entry
                            .attachments
                            .iter()
                            .find(|r| r.path == path && !r.hash.is_empty())
                    });
                    if let Some(existing) = existing_ref {
                        existing.clone()
                    } else {
                        BinaryRef {
                            path,
                            source: "local".to_string(),
                            hash: String::new(),
                            mime_type: String::new(),
                            size: 0,
                            uploaded_at: None,
                            deleted: false,
                        }
                    }
                })
                .collect();

            // Build extra fields
            let mut extra: HashMap<String, serde_json::Value> = HashMap::new();
            for (key, value) in &parsed.frontmatter {
                if ![
                    "title",
                    "link",
                    "links",
                    "link_of",
                    "part_of",
                    "contents",
                    "attachments",
                    "audience",
                    "description",
                ]
                .contains(&key.as_str())
                {
                    if let Ok(json) = serde_json::to_value(value) {
                        extra.insert(key.clone(), json);
                    }
                }
            }

            let modified_at = file_mtime.unwrap_or_else(|| crate::time::now_timestamp_millis());

            let filename = std::path::Path::new(&canonical_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let metadata = FileMetadata {
                filename,
                title,
                link: parsed
                    .frontmatter
                    .get("link")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                links: parsed.frontmatter.get("links").and_then(|v| {
                    v.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str())
                            .map(|raw_value| {
                                let parsed_link = link_parser::parse_link(raw_value);
                                link_parser::to_canonical_with_link_format(
                                    &parsed_link,
                                    Path::new(&canonical_path),
                                    link_format_hint,
                                )
                            })
                            .collect()
                    })
                }),
                link_of: parsed.frontmatter.get("link_of").and_then(|v| {
                    v.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str())
                            .map(|raw_value| {
                                let parsed_link = link_parser::parse_link(raw_value);
                                link_parser::to_canonical_with_link_format(
                                    &parsed_link,
                                    Path::new(&canonical_path),
                                    link_format_hint,
                                )
                            })
                            .collect()
                    })
                }),
                part_of: parent_path.clone(),
                contents,
                attachments,
                attachment: parsed
                    .frontmatter
                    .get("attachment")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                attachment_of: parsed.frontmatter.get("attachment_of").and_then(|v| {
                    v.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                }),
                deleted: false,
                audience: file_audience,
                description,
                extra,
                modified_at,
            };

            files_to_add.push((canonical_path.clone(), metadata));

            for child in node.children.iter().rev() {
                stack.push((child, Some(canonical_path.clone())));
            }
        }

        // Populate CRDT
        let file_count = files_to_add.len();
        let updated_count = files_updated_from_disk.len();

        for (path, metadata) in &files_to_add {
            if let Err(e) = self.workspace_crdt.set_file(path, metadata.clone()) {
                log::warn!(
                    "[InitializeWorkspaceCrdt] Failed to set file {}: {:?}",
                    path,
                    e
                );
            }
        }

        self.workspace_crdt.save().map_err(map_err)?;

        let msg = if updated_count > 0 {
            if audience.is_some() {
                format!(
                    "{} files populated, {} updated from disk (audience filtered)",
                    file_count, updated_count
                )
            } else {
                format!(
                    "{} files populated, {} updated from disk",
                    file_count, updated_count
                )
            }
        } else if audience.is_some() {
            format!("{} files populated (audience filtered)", file_count)
        } else {
            format!("{} files populated", file_count)
        };
        log::info!("[InitializeWorkspaceCrdt] {}", msg);

        Ok(
            serde_json::json!({ "message": msg, "file_count": file_count, "updated_count": updated_count }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diaryx_core::fs::{InMemoryFileSystem, SyncToAsyncFs};

    use crate::MemoryStorage;

    type TestFs = SyncToAsyncFs<InMemoryFileSystem>;

    fn make_plugin() -> SyncPlugin<TestFs> {
        let fs = SyncToAsyncFs::new(InMemoryFileSystem::new());
        let storage = Arc::new(MemoryStorage::new());
        SyncPlugin::new(fs, storage)
    }

    #[test]
    fn test_sync_plugin_new() {
        let plugin = make_plugin();
        assert_eq!(plugin.id(), PluginId("diaryx.sync".into()));
    }

    #[test]
    fn test_sync_plugin_accessors() {
        let plugin = make_plugin();

        // All accessors return valid Arc handles
        let _wc = plugin.workspace_crdt();
        let _bd = plugin.body_docs();
        let _sh = plugin.sync_handler();
        let _sm = plugin.sync_manager();
        let _st = plugin.storage();
    }

    #[test]
    fn test_sync_manifest_shows_free_plan_and_plus_upgrade_states() {
        let manifest = sync_plugin_manifest();
        let settings_fields = manifest
            .ui
            .iter()
            .find_map(|entry| match entry {
                UiContribution::SettingsTab { id, fields, .. } if id == "sync-settings" => {
                    Some(fields)
                }
                _ => None,
            })
            .expect("sync settings tab");

        assert!(matches!(
            settings_fields.first(),
            Some(SettingsField::AuthStatus { .. })
        ));
        assert!(settings_fields.iter().any(|field| matches!(
            field,
            SettingsField::Conditional { condition, fields }
                if condition == "not_plus"
                    && fields.iter().any(|entry| matches!(
                        entry,
                        SettingsField::Section { label, .. } if label == "Free Plan"
                    ))
                    && fields.iter().any(|entry| matches!(
                        entry,
                        SettingsField::Section { label, .. } if label == "Plus Unlocks"
                    ))
        )));
        assert!(settings_fields.iter().any(|field| matches!(
            field,
            SettingsField::UpgradeBanner { feature, .. } if feature == "More Sync"
        )));
        assert!(settings_fields.iter().any(|field| matches!(
            field,
            SettingsField::Conditional { condition, fields }
                if condition == "plus"
                    && fields.iter().any(|entry| matches!(
                        entry,
                        SettingsField::Button { command, .. } if command == "GetProviderStatus"
                    ))
        )));
    }

    #[tokio::test]
    async fn test_sync_plugin_init() {
        let plugin = make_plugin();

        let ctx = PluginContext::new(Some(PathBuf::from("/workspace")), LinkFormat::MarkdownRoot);
        Plugin::init(&plugin, &ctx).await.unwrap();

        assert_eq!(
            *plugin.workspace_root.read().unwrap(),
            Some(PathBuf::from("/workspace"))
        );
        assert_eq!(
            *plugin.link_format.read().unwrap(),
            LinkFormat::MarkdownRoot
        );
    }

    #[tokio::test]
    async fn test_sync_plugin_file_metadata_roundtrip() {
        let plugin = make_plugin();

        // Set a file
        let metadata = FileMetadata {
            filename: "test.md".to_string(),
            title: Some("Test".to_string()),
            ..Default::default()
        };
        let params = serde_json::json!({
            "path": "test.md",
            "metadata": metadata,
        });
        let result: Result<JsonValue, PluginError> = plugin.dispatch("SetCrdtFile", params).await;
        assert!(result.is_ok());

        // Get it back
        let params = serde_json::json!({ "path": "test.md" });
        let result: JsonValue = plugin.dispatch("GetCrdtFile", params).await.unwrap();
        let file: Option<FileMetadata> = serde_json::from_value(result).unwrap();
        assert!(file.is_some());
        assert_eq!(file.unwrap().title, Some("Test".to_string()));
    }

    #[tokio::test]
    async fn test_sync_plugin_body_content_roundtrip() {
        let plugin = make_plugin();

        // Set body content
        let params = serde_json::json!({
            "doc_name": "test.md",
            "content": "Hello world",
        });
        let _: JsonValue = plugin.dispatch("SetBodyContent", params).await.unwrap();

        // Get it back
        let params = serde_json::json!({ "doc_name": "test.md" });
        let result: JsonValue = plugin.dispatch("GetBodyContent", params).await.unwrap();
        assert_eq!(result["content"], "Hello world");
    }

    #[tokio::test]
    async fn test_sync_plugin_unknown_command() {
        let plugin = make_plugin();

        let result: Result<JsonValue, PluginError> =
            plugin.dispatch("NonExistentCommand", JsonValue::Null).await;
        assert!(result.is_err());
    }
}
