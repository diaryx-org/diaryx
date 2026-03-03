//! JSON protocol types shared between the host and guest WASM plugins.
//!
//! Guest plugins export functions that receive and return these types
//! serialized as JSON. This module defines the contract — any language
//! with an Extism PDK can implement a compatible guest.

use std::collections::HashMap;

use diaryx_core::plugin::permissions::PluginPermissions;
use serde::{Deserialize, Serialize};

/// Plugin-declared default permissions and human-readable reasons.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuestRequestedPermissions {
    /// Default permission rules to apply at install time.
    #[serde(default)]
    pub defaults: PluginPermissions,
    /// Why each permission is needed, keyed by permission field name.
    #[serde(default)]
    pub reasons: HashMap<String, String>,
}

/// Manifest returned by the guest's exported `manifest` function.
///
/// The host calls `manifest("")` at load time and caches the result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestManifest {
    /// Unique plugin identifier (e.g., `"com.example.word-count"`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// Short description of what this plugin does.
    pub description: String,
    /// Capability strings this plugin requests.
    ///
    /// Known values: `"file_events"`, `"workspace_events"`, `"custom_commands"`.
    pub capabilities: Vec<String>,
    /// Serialized [`UiContribution`](diaryx_core::plugin::UiContribution) values.
    ///
    /// The host deserializes each element into the core `UiContribution` enum.
    #[serde(default)]
    pub ui: Vec<serde_json::Value>,
    /// Custom command names this plugin handles (e.g., `["word-count"]`).
    #[serde(default)]
    pub commands: Vec<String>,
    /// CLI subcommand declarations (deserialized into `CliCommand` by the host).
    #[serde(default)]
    pub cli: Vec<serde_json::Value>,
    /// Optional default permission request + rationale shown during install.
    #[serde(default)]
    pub requested_permissions: Option<GuestRequestedPermissions>,
}

/// Event sent to the guest's `on_event` function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestEvent {
    /// Event type identifier.
    ///
    /// Known values:
    /// - `"workspace_opened"`, `"workspace_closed"`, `"workspace_changed"`, `"workspace_committed"`
    /// - `"file_saved"`, `"file_created"`, `"file_deleted"`, `"file_moved"`
    pub event_type: String,
    /// Event-specific payload (varies by event type).
    pub payload: serde_json::Value,
}

/// Command request sent to the guest's `handle_command` function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    /// Command name (matches one of the guest's declared commands).
    pub command: String,
    /// Command parameters.
    pub params: serde_json::Value,
}

/// Response returned by the guest from `handle_command`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    /// Whether the command succeeded.
    pub success: bool,
    /// Result data (present on success).
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    /// Error message (present on failure).
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_manifest_roundtrip() {
        let manifest = GuestManifest {
            id: "com.example.test".into(),
            name: "Test Plugin".into(),
            version: "0.1.0".into(),
            description: "A test plugin".into(),
            capabilities: vec!["file_events".into(), "custom_commands".into()],
            ui: vec![],
            commands: vec!["do-thing".into()],
            cli: vec![],
            requested_permissions: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: GuestManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "com.example.test");
        assert_eq!(parsed.commands, vec!["do-thing"]);
    }

    #[test]
    fn command_response_roundtrip() {
        let resp = CommandResponse {
            success: true,
            data: Some(serde_json::json!({"count": 42})),
            error: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: CommandResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.data.unwrap()["count"], 42);
    }

    #[test]
    fn guest_event_roundtrip() {
        let event = GuestEvent {
            event_type: "file_saved".into(),
            payload: serde_json::json!({"path": "2024/01/entry.md"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: GuestEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type, "file_saved");
    }
}
