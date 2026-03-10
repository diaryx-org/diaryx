//! JSON protocol types shared between the host and guest WASM plugins.
//!
//! Guest plugins export functions that receive and return these types
//! serialized as JSON. This module defines the contract — any language
//! with an Extism PDK can implement a compatible guest.

use std::collections::HashMap;

use diaryx_core::plugin::permissions::PluginPermissions;
use serde::{Deserialize, Serialize};

/// The current protocol version supported by this host.
pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

/// The minimum protocol version the host can still load.
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u32 = 1;

fn default_protocol_version() -> u32 {
    1
}

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
    /// Protocol version this guest was built against.
    ///
    /// Omitting defaults to 1 for backward compatibility with existing plugins.
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u32,
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
    /// Optional structured error code from the guest (e.g., `"permission_denied"`, `"config_error"`).
    #[serde(default)]
    pub error_code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_manifest_roundtrip() {
        let manifest = GuestManifest {
            protocol_version: 1,
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
        assert_eq!(parsed.protocol_version, 1);
        assert_eq!(parsed.id, "com.example.test");
        assert_eq!(parsed.commands, vec!["do-thing"]);
    }

    #[test]
    fn guest_manifest_defaults_protocol_version_to_1() {
        // Simulates an old plugin that omits protocol_version.
        let json =
            r#"{"id":"test","name":"T","version":"1.0","description":"d","capabilities":[]}"#;
        let m: GuestManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.protocol_version, 1);
    }

    #[test]
    fn guest_manifest_explicit_protocol_version() {
        let json = r#"{"protocol_version":2,"id":"test","name":"T","version":"1.0","description":"d","capabilities":[]}"#;
        let m: GuestManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.protocol_version, 2);
    }

    #[test]
    fn command_response_roundtrip() {
        let resp = CommandResponse {
            success: true,
            data: Some(serde_json::json!({"count": 42})),
            error: None,
            error_code: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: CommandResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.data.unwrap()["count"], 42);
    }

    #[test]
    fn command_response_without_error_code() {
        // Simulates an old plugin that omits error_code.
        let json = r#"{"success":false,"error":"oops"}"#;
        let resp: CommandResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert_eq!(resp.error.as_deref(), Some("oops"));
        assert!(resp.error_code.is_none());
    }

    #[test]
    fn command_response_with_error_code() {
        let json = r#"{"success":false,"error":"denied","error_code":"permission_denied"}"#;
        let resp: CommandResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert_eq!(resp.error_code.as_deref(), Some("permission_denied"));
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
