//! JSON protocol types shared between the host and guest WASM plugins.
//!
//! These are the canonical guest-side definitions matching the host's
//! `diaryx_extism::protocol` module. By depending on this SDK, plugins no
//! longer need to maintain their own copies of these types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The current protocol version supported by this SDK.
pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

/// The minimum protocol version the host can still load.
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u32 = 1;

fn default_protocol_version() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// Plugin-declared default permissions and human-readable reasons.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuestRequestedPermissions {
    /// Default permission rules to apply at install time.
    #[serde(default)]
    pub defaults: serde_json::Value,
    /// Why each permission is needed, keyed by permission field name.
    #[serde(default)]
    pub reasons: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Manifest returned by the guest's exported `manifest` function.
///
/// The host calls `manifest("")` at load time and caches the result.
///
/// Use [`GuestManifest::new`] to construct — only `id`, `name`, `version`,
/// `description`, and `capabilities` are required. All other fields default
/// to empty/`None` and can be set via builder-style methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GuestManifest {
    /// Protocol version this guest was built against.
    ///
    /// Omitting defaults to 1 for backward compatibility with existing plugins.
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u32,
    /// Unique plugin identifier (e.g., `"diaryx.myplugin"`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// Short description of what this plugin does.
    pub description: String,
    /// Capability strings this plugin requests.
    ///
    /// Known values: `"file_events"`, `"workspace_events"`, `"custom_commands"`,
    /// `"editor_extension"`, `"media_transcoder"`, `"command"`, `"lifecycle"`.
    pub capabilities: Vec<String>,
    /// Serialized UI contribution values.
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
    /// Supported conversion pairs for `media_transcoder` capability (e.g. `["heic:jpeg"]`).
    #[serde(default)]
    pub conversions: Vec<String>,
    /// Minimum Diaryx version required to run this plugin (e.g. `"1.4.0"`).
    ///
    /// The host checks this at load time and rejects the plugin with a
    /// user-friendly message when the running app is too old. `None` means
    /// the plugin is compatible with any version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<String>,
    /// Server-side API endpoints this plugin interacts with.
    ///
    /// Declarative metadata: documents the contract between plugin and server.
    /// The server implements these routes as standard handlers; the client plugin
    /// calls them. Used for documentation, validation, and future marketplace tooling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server_functions: Vec<ServerFunctionDecl>,
}

impl GuestManifest {
    /// Create a manifest with the required fields. Optional fields default to
    /// empty and can be set with the builder methods below.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: description.into(),
            capabilities,
            ui: vec![],
            commands: vec![],
            cli: vec![],
            requested_permissions: None,
            conversions: vec![],
            min_app_version: None,
            server_functions: vec![],
        }
    }

    /// Set UI contributions.
    pub fn ui(mut self, ui: Vec<serde_json::Value>) -> Self {
        self.ui = ui;
        self
    }

    /// Set custom command names.
    pub fn commands(mut self, commands: Vec<String>) -> Self {
        self.commands = commands;
        self
    }

    /// Set CLI subcommand declarations.
    pub fn cli(mut self, cli: Vec<serde_json::Value>) -> Self {
        self.cli = cli;
        self
    }

    /// Set requested permissions.
    pub fn requested_permissions(mut self, perms: GuestRequestedPermissions) -> Self {
        self.requested_permissions = Some(perms);
        self
    }

    /// Set media transcoder conversion pairs (e.g. `["heic:jpeg"]`).
    pub fn conversions(mut self, conversions: Vec<String>) -> Self {
        self.conversions = conversions;
        self
    }

    /// Set the minimum Diaryx version required (e.g. `"1.4.0"`).
    pub fn min_app_version(mut self, version: impl Into<String>) -> Self {
        self.min_app_version = Some(version.into());
        self
    }

    /// Declare server-side API endpoints this plugin interacts with.
    pub fn server_functions(mut self, fns: Vec<ServerFunctionDecl>) -> Self {
        self.server_functions = fns;
        self
    }
}

// ---------------------------------------------------------------------------
// Server functions
// ---------------------------------------------------------------------------

/// Declares a server-side API endpoint this plugin interacts with.
///
/// This is declarative metadata — the server implements these routes as
/// standard Rust handlers, and the client plugin calls them. The declaration
/// documents the plugin↔server contract and enables tooling validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerFunctionDecl {
    /// Logical name for this function (e.g. `"sync_ws"`, `"put_html"`).
    pub name: String,
    /// HTTP method or `"WS"` for WebSocket (e.g. `"GET"`, `"PUT"`, `"POST"`, `"WS"`).
    pub method: String,
    /// URL path with optional `{param}` segments (e.g. `"/namespaces/{id}/objects/{key}"`).
    pub path: String,
    /// Human-readable description of what this endpoint does.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

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
    /// Optional structured error code (e.g., `"permission_denied"`, `"config_error"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl CommandResponse {
    /// Create a successful response with data.
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            error_code: None,
        }
    }

    /// Create a successful response with no data.
    pub fn ok_empty() -> Self {
        Self {
            success: true,
            data: None,
            error: None,
            error_code: None,
        }
    }

    /// Create an error response.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
            error_code: None,
        }
    }

    /// Create an error response with a structured error code.
    pub fn err_with_code(message: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
            error_code: Some(code.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_manifest_roundtrip() {
        let manifest = GuestManifest::new(
            "diaryx.test",
            "Test Plugin",
            "0.1.0",
            "A test plugin",
            vec!["custom_commands".into()],
        )
        .commands(vec!["do-thing".into()]);
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: GuestManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "diaryx.test");
        assert_eq!(parsed.commands, vec!["do-thing"]);
    }

    #[test]
    fn manifest_defaults_protocol_version() {
        let json =
            r#"{"id":"test","name":"T","version":"1.0","description":"d","capabilities":[]}"#;
        let m: GuestManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.protocol_version, 1);
    }

    #[test]
    fn command_response_helpers() {
        let ok = CommandResponse::ok(serde_json::json!({"count": 42}));
        assert!(ok.success);
        assert_eq!(ok.data.unwrap()["count"], 42);

        let err = CommandResponse::err("oops");
        assert!(!err.success);
        assert_eq!(err.error.as_deref(), Some("oops"));

        let err_code = CommandResponse::err_with_code("denied", "permission_denied");
        assert_eq!(err_code.error_code.as_deref(), Some("permission_denied"));
    }

    #[test]
    fn command_response_without_error_code() {
        let json = r#"{"success":false,"error":"oops"}"#;
        let resp: CommandResponse = serde_json::from_str(json).unwrap();
        assert!(!resp.success);
        assert!(resp.error_code.is_none());
    }

    #[test]
    fn server_function_decl_roundtrip() {
        let decl = ServerFunctionDecl {
            name: "sync_ws".into(),
            method: "WS".into(),
            path: "/namespaces/{id}/sync".into(),
            description: "CRDT relay WebSocket".into(),
        };
        let json = serde_json::to_string(&decl).unwrap();
        let parsed: ServerFunctionDecl = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "sync_ws");
        assert_eq!(parsed.method, "WS");
    }

    #[test]
    fn manifest_with_server_functions() {
        let manifest = GuestManifest::new("diaryx.sync", "Sync", "1.0.0", "Sync plugin", vec![])
            .server_functions(vec![ServerFunctionDecl {
                name: "sync_ws".into(),
                method: "WS".into(),
                path: "/namespaces/{id}/sync".into(),
                description: "CRDT relay".into(),
            }]);
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: GuestManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.server_functions.len(), 1);
        assert_eq!(parsed.server_functions[0].name, "sync_ws");
    }

    #[test]
    fn manifest_server_functions_defaults_empty() {
        let json =
            r#"{"id":"test","name":"T","version":"1.0","description":"d","capabilities":[]}"#;
        let m: GuestManifest = serde_json::from_str(json).unwrap();
        assert!(m.server_functions.is_empty());
    }
}
