//! Adapter wrapping an Extism WASM plugin as a diaryx_core plugin.
//!
//! [`ExtismPluginAdapter`] loads a guest WASM module, caches its manifest,
//! and implements [`Plugin`], [`WorkspacePlugin`], and [`FilePlugin`] by
//! calling the guest's exported functions with JSON payloads.

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use diaryx_core::plugin::{
    FileCreatedEvent, FileDeletedEvent, FileMovedEvent, FilePlugin, FileSavedEvent, Plugin,
    PluginCapability, PluginContext, PluginError, PluginId, PluginManifest, UiContribution,
    WorkspaceChangedEvent, WorkspaceClosedEvent, WorkspaceCommittedEvent, WorkspaceOpenedEvent,
    WorkspacePlugin,
};

use crate::protocol::{CommandRequest, CommandResponse, GuestEvent, GuestManifest};

/// Wraps an `extism::Plugin` and implements the diaryx_core plugin traits.
///
/// The guest WASM module communicates with the host through JSON payloads
/// defined in [`crate::protocol`].
pub struct ExtismPluginAdapter {
    /// The extism::Plugin is `!Send`, so we wrap in Mutex for trait compatibility.
    inner: Mutex<extism::Plugin>,
    /// Cached manifest (parsed once at load time).
    manifest: PluginManifest,
    /// Plugin config stored as a JSON sidecar.
    config: Mutex<JsonValue>,
    /// Path to the config file on disk.
    config_path: PathBuf,
}

// SAFETY: extism::Plugin is !Send because it contains raw pointers to the WASM
// runtime. However, we only access it through a Mutex, serializing all calls.
// Each call is short-lived (JSON in → JSON out) so this is safe in practice.
unsafe impl Send for ExtismPluginAdapter {}
unsafe impl Sync for ExtismPluginAdapter {}

impl ExtismPluginAdapter {
    /// Create a new adapter from an already-initialized Extism plugin.
    ///
    /// The `guest_manifest` should have been obtained by calling the guest's
    /// `manifest` export. The `config_path` points to the JSON sidecar file
    /// where plugin configuration is persisted.
    pub fn new(
        plugin: extism::Plugin,
        guest_manifest: GuestManifest,
        config: JsonValue,
        config_path: PathBuf,
    ) -> Self {
        let manifest = convert_guest_manifest(&guest_manifest);
        Self {
            inner: Mutex::new(plugin),
            manifest,
            config: Mutex::new(config),
            config_path,
        }
    }

    /// Call a guest-exported function with a JSON input, returning the output string.
    pub fn call_guest(&self, func: &str, input: &str) -> Result<String, PluginError> {
        let mut plugin = self
            .inner
            .lock()
            .map_err(|e| PluginError::Other(format!("Failed to lock extism plugin: {e}")))?;
        let output = plugin
            .call::<&str, &[u8]>(func, input)
            .map_err(|e| PluginError::Other(format!("Extism call `{func}` failed: {e}")))?;
        Ok(String::from_utf8_lossy(output).into_owned())
    }

    /// Call a guest-exported function with binary input, returning raw bytes.
    ///
    /// Used for hot-path binary exports (sync messages, CRDT updates).
    pub fn call_guest_binary(&self, func: &str, input: &[u8]) -> Result<Vec<u8>, PluginError> {
        let mut plugin = self
            .inner
            .lock()
            .map_err(|e| PluginError::Other(format!("Failed to lock extism plugin: {e}")))?;
        let output = plugin
            .call::<&[u8], &[u8]>(func, input)
            .map_err(|e| PluginError::Other(format!("Extism call `{func}` failed: {e}")))?;
        Ok(output.to_vec())
    }

    /// Call a guest function, ignoring the output. Logs errors but doesn't propagate.
    fn call_guest_fire_and_forget(&self, func: &str, input: &str) {
        if let Err(e) = self.call_guest(func, input) {
            log::warn!("Extism plugin {}: {e}", self.manifest.id);
        }
    }

    /// Send an event to the guest's `on_event` export.
    fn send_event(&self, event: &GuestEvent) {
        match serde_json::to_string(event) {
            Ok(json) => self.call_guest_fire_and_forget("on_event", &json),
            Err(e) => log::warn!(
                "Extism plugin {}: failed to serialize event: {e}",
                self.manifest.id
            ),
        }
    }

    /// Persist the current config to the sidecar file.
    fn persist_config(&self) -> Result<(), PluginError> {
        let config = self
            .config
            .lock()
            .map_err(|e| PluginError::Other(format!("Failed to lock config: {e}")))?;
        let json = serde_json::to_string_pretty(&*config)
            .map_err(|e| PluginError::Other(format!("Failed to serialize config: {e}")))?;
        std::fs::write(&self.config_path, json)
            .map_err(|e| PluginError::Other(format!("Failed to write config file: {e}")))?;
        Ok(())
    }
}

// ============================================================================
// Plugin trait
// ============================================================================

#[async_trait]
impl Plugin for ExtismPluginAdapter {
    fn id(&self) -> PluginId {
        self.manifest.id.clone()
    }

    fn manifest(&self) -> PluginManifest {
        self.manifest.clone()
    }

    async fn init(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let ctx_json = serde_json::json!({
            "workspace_root": ctx.workspace_root,
        });
        let input = serde_json::to_string(&ctx_json)
            .map_err(|e| PluginError::InitFailed(format!("Failed to serialize context: {e}")))?;
        // If the guest doesn't export `init`, that's fine — it's optional.
        let _ = self.call_guest("init", &input);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), PluginError> {
        // WASM plugins don't hold resources that need explicit release.
        Ok(())
    }
}

// ============================================================================
// WorkspacePlugin trait
// ============================================================================

#[async_trait]
impl WorkspacePlugin for ExtismPluginAdapter {
    async fn on_workspace_opened(&self, event: &WorkspaceOpenedEvent) {
        self.send_event(&GuestEvent {
            event_type: "workspace_opened".into(),
            payload: serde_json::json!({
                "workspace_root": event.workspace_root,
            }),
        });
    }

    async fn on_workspace_closed(&self, event: &WorkspaceClosedEvent) {
        self.send_event(&GuestEvent {
            event_type: "workspace_closed".into(),
            payload: serde_json::json!({
                "workspace_root": event.workspace_root,
            }),
        });
    }

    async fn on_workspace_changed(&self, event: &WorkspaceChangedEvent) {
        self.send_event(&GuestEvent {
            event_type: "workspace_changed".into(),
            payload: serde_json::json!({
                "workspace_root": event.workspace_root,
                "changed_paths": event.changed_paths,
            }),
        });
    }

    async fn on_workspace_committed(&self, event: &WorkspaceCommittedEvent) {
        self.send_event(&GuestEvent {
            event_type: "workspace_committed".into(),
            payload: serde_json::json!({
                "workspace_root": event.workspace_root,
            }),
        });
    }

    async fn handle_command(
        &self,
        cmd: &str,
        params: JsonValue,
    ) -> Option<Result<JsonValue, PluginError>> {
        // Only handle commands this plugin declared.
        let declared = self
            .manifest
            .capabilities
            .iter()
            .any(|c| matches!(c, PluginCapability::CustomCommands { commands } if commands.contains(&cmd.to_string())));
        if !declared {
            return None;
        }

        let request = CommandRequest {
            command: cmd.to_string(),
            params,
        };
        let input = match serde_json::to_string(&request) {
            Ok(json) => json,
            Err(e) => {
                return Some(Err(PluginError::CommandError(format!(
                    "Failed to serialize command request: {e}"
                ))));
            }
        };

        match self.call_guest("handle_command", &input) {
            Ok(output) => match serde_json::from_str::<CommandResponse>(&output) {
                Ok(resp) => {
                    if resp.success {
                        Some(Ok(resp.data.unwrap_or(JsonValue::Null)))
                    } else {
                        Some(Err(PluginError::CommandError(
                            resp.error.unwrap_or_else(|| "Unknown error".into()),
                        )))
                    }
                }
                Err(e) => Some(Err(PluginError::CommandError(format!(
                    "Failed to parse command response: {e}"
                )))),
            },
            Err(e) => Some(Err(e)),
        }
    }

    async fn get_config(&self) -> Option<JsonValue> {
        let config = self.config.lock().ok()?;
        if config.is_null() || config.as_object().is_some_and(|m| m.is_empty()) {
            None
        } else {
            Some(config.clone())
        }
    }

    async fn set_config(&self, config: JsonValue) -> Result<(), PluginError> {
        {
            let mut current = self
                .config
                .lock()
                .map_err(|e| PluginError::Other(format!("Failed to lock config: {e}")))?;
            *current = config.clone();
        }
        self.persist_config()?;

        // Notify the guest of the config change.
        let input = serde_json::to_string(&config).unwrap_or_default();
        let _ = self.call_guest("set_config", &input);
        Ok(())
    }
}

// ============================================================================
// FilePlugin trait
// ============================================================================

#[async_trait]
impl FilePlugin for ExtismPluginAdapter {
    async fn on_file_saved(&self, event: &FileSavedEvent) {
        self.send_event(&GuestEvent {
            event_type: "file_saved".into(),
            payload: serde_json::json!({ "path": event.path }),
        });
    }

    async fn on_file_created(&self, event: &FileCreatedEvent) {
        self.send_event(&GuestEvent {
            event_type: "file_created".into(),
            payload: serde_json::json!({ "path": event.path }),
        });
    }

    async fn on_file_deleted(&self, event: &FileDeletedEvent) {
        self.send_event(&GuestEvent {
            event_type: "file_deleted".into(),
            payload: serde_json::json!({ "path": event.path }),
        });
    }

    async fn on_file_moved(&self, event: &FileMovedEvent) {
        self.send_event(&GuestEvent {
            event_type: "file_moved".into(),
            payload: serde_json::json!({
                "old_path": event.old_path,
                "new_path": event.new_path,
            }),
        });
    }
}

// ============================================================================
// Manifest conversion
// ============================================================================

/// Convert a [`GuestManifest`] to a [`PluginManifest`].
fn convert_guest_manifest(guest: &GuestManifest) -> PluginManifest {
    let capabilities = guest
        .capabilities
        .iter()
        .filter_map(|cap| match cap.as_str() {
            "file_events" => Some(PluginCapability::FileEvents),
            "workspace_events" => Some(PluginCapability::WorkspaceEvents),
            "custom_commands" => Some(PluginCapability::CustomCommands {
                commands: guest.commands.clone(),
            }),
            "editor_extension" => Some(PluginCapability::EditorExtension),
            other => {
                log::warn!("Unknown capability: {other}");
                None
            }
        })
        .collect();

    let ui: Vec<UiContribution> = guest
        .ui
        .iter()
        .filter_map(|val| {
            serde_json::from_value(val.clone())
                .map_err(|e| {
                    log::warn!("Plugin {}: failed to parse UI contribution: {e}", guest.id);
                    e
                })
                .ok()
        })
        .collect();

    PluginManifest {
        id: PluginId(guest.id.clone()),
        name: guest.name.clone(),
        version: guest.version.clone(),
        description: guest.description.clone(),
        capabilities,
        ui,
    }
}
