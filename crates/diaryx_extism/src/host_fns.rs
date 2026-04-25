//! Host functions exposed to guest WASM plugins.
//!
//! These functions give guest plugins controlled, sandboxed access to the
//! Diaryx environment. They are registered with the Extism plugin via
//! [`PluginBuilder`](extism::PluginBuilder).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Local, SecondsFormat};
use diaryx_core::fs::AsyncFileSystem;
use diaryx_core::plugin::permissions::PermissionType;
use extism::{CurrentPlugin, Error as ExtismError, UserData, Val, ValType};

use crate::permission_checker::DenyAllPermissionChecker;

/// Trait for persisting plugin state (CRDT snapshots, config, etc.).
///
/// Implementations might use SQLite on native or IndexedDB on web.
pub trait PluginStorage: Send + Sync {
    /// Load a value by key.
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    /// Store a value by key.
    fn set(&self, key: &str, data: &[u8]);
    /// Delete a value by key.
    fn delete(&self, key: &str);
}

/// Trait for persisting plugin secrets separately from normal plugin state.
pub trait PluginSecretStore: Send + Sync {
    /// Load a secret by key.
    fn get(&self, key: &str) -> Option<String>;
    /// Store a secret by key.
    fn set(&self, key: &str, value: &str);
    /// Delete a secret by key.
    fn delete(&self, key: &str);
}

/// Trait for emitting events from plugins to the host application.
pub trait EventEmitter: Send + Sync {
    /// Emit an event (JSON payload) to the host.
    fn emit(&self, event_json: &str);
}

/// Trait for handling plugin-initiated websocket transport requests.
pub trait WebSocketBridge: Send + Sync {
    /// Handle a serialized websocket request and return a serialized response.
    fn request(&self, request_json: &str) -> Result<String, String>;
}

/// Trait for plugin-to-plugin command dispatch mediated by the host.
pub trait PluginCommandBridge: Send + Sync {
    /// Execute a command on another plugin and return the plugin's raw JSON data.
    fn call(
        &self,
        caller_plugin_id: &str,
        plugin_id: &str,
        command: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// Trait for exposing generic host runtime context to plugins.
pub trait RuntimeContextProvider: Send + Sync {
    /// Return runtime context for the caller plugin.
    fn get_context(&self, plugin_id: &str) -> serde_json::Value;
}

/// Metadata for a single object in a namespace.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamespaceObjectMeta {
    #[serde(default)]
    pub namespace_id: Option<String>,
    pub key: String,
    #[serde(default)]
    pub r2_key: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
    #[serde(default)]
    pub content_hash: Option<String>,
}

/// Entry returned by `list_namespaces` — mirrors the server's `NamespaceResponse`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamespaceEntry {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// A single entry in a batch-get response.
#[derive(Debug, Clone)]
pub struct BatchGetEntry {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// Result of a batch object download.
#[derive(Debug, Clone, Default)]
pub struct BatchGetResult {
    pub objects: std::collections::HashMap<String, BatchGetEntry>,
    pub errors: std::collections::HashMap<String, String>,
}

/// Trait for namespace object operations (upload, delete, list, sync audience).
///
/// Implementations talk to the sync server — via `proxyFetch` on browser,
/// via `ureq` on native.
pub trait NamespaceProvider: Send + Sync {
    fn create_namespace(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Result<NamespaceEntry, String>;
    fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
    ) -> Result<(), String>;
    fn get_object(&self, ns_id: &str, key: &str) -> Result<Vec<u8>, String>;
    fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String>;
    fn list_objects(
        &self,
        ns_id: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<NamespaceObjectMeta>, String>;
    /// Sync an audience's gate stack on the server. `gates` is a JSON array
    /// of `GateRecord` objects (`{ "kind": "link" }` or
    /// `{ "kind": "password", ... }`); empty array = public.
    fn sync_audience(
        &self,
        ns_id: &str,
        audience: &str,
        gates: &serde_json::Value,
    ) -> Result<(), String>;

    /// List the names of all audiences configured on the server for a
    /// namespace. Used by the publish plugin's strict-sync pass.
    fn list_audiences(&self, ns_id: &str) -> Result<Vec<String>, String>;

    /// Delete an audience from the server (and its tagged objects). Used by
    /// the publish plugin's strict-sync pass when an audience has been
    /// removed from the workspace file.
    fn delete_audience(&self, ns_id: &str, audience: &str) -> Result<(), String>;

    /// Download multiple objects in a single request.
    fn get_objects_batch(&self, ns_id: &str, keys: &[String]) -> Result<BatchGetResult, String>;

    /// List all namespaces owned by the authenticated user.
    fn list_namespaces(&self) -> Result<Vec<NamespaceEntry>, String>;
}

/// Parse a `multipart/mixed` response body into a [`BatchGetResult`].
///
/// Each part is identified by its `Content-Disposition: attachment; filename="<key>"`
/// header. Parts with an `X-Batch-Error: true` header are treated as per-key errors.
pub fn parse_multipart_batch(body: &[u8], boundary: &str) -> BatchGetResult {
    let mut result = BatchGetResult::default();
    let delim = format!("--{boundary}");
    let closing = format!("--{boundary}--");

    // Split body on boundary markers.
    let delim_bytes = delim.as_bytes();
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut start = 0;

    while let Some(pos) = memmem(body, start, delim_bytes) {
        if start > 0 {
            // Trim trailing \r\n before boundary.
            let end = if pos >= 2 && body[pos - 2] == b'\r' && body[pos - 1] == b'\n' {
                pos - 2
            } else {
                pos
            };
            parts.push(&body[start..end]);
        }
        start = pos + delim_bytes.len();
        // Skip \r\n after boundary line.
        if start < body.len() && body[start] == b'\r' {
            start += 1;
        }
        if start < body.len() && body[start] == b'\n' {
            start += 1;
        }
        // Check for closing boundary (--boundary--).
        if start >= 2 && body[start - 2..start].starts_with(b"--") {
            break;
        }
    }

    for part in parts {
        // Split headers from body at the first \r\n\r\n.
        let header_end = match memmem(part, 0, b"\r\n\r\n") {
            Some(pos) => pos,
            None => continue,
        };
        let header_section = &part[..header_end];
        let body_section = &part[header_end + 4..];

        let headers_str = String::from_utf8_lossy(header_section);
        let mut filename: Option<String> = None;
        let mut content_type = "application/octet-stream".to_string();
        let mut is_error = false;

        for line in headers_str.split("\r\n") {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-disposition:") {
                if let Some(pos) = line.find("filename=\"") {
                    let start = pos + 10;
                    if let Some(end) = line[start..].find('\"') {
                        filename = Some(line[start..start + end].replace("\\\"", "\""));
                    }
                }
            } else if lower.starts_with("content-type:") {
                content_type = line["content-type:".len()..].trim().to_string();
            } else if lower.starts_with("x-batch-error:") {
                is_error = lower.contains("true");
            }
        }

        if let Some(key) = filename {
            if is_error {
                let msg = String::from_utf8_lossy(body_section).to_string();
                result.errors.insert(key, msg);
            } else {
                result.objects.insert(
                    key,
                    BatchGetEntry {
                        bytes: body_section.to_vec(),
                        mime_type: content_type,
                    },
                );
            }
        }
    }

    result
}

/// Find the first occurrence of `needle` in `haystack` starting from `offset`.
fn memmem(haystack: &[u8], offset: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || offset + needle.len() > haystack.len() {
        return None;
    }
    haystack[offset..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + offset)
}

#[cfg(test)]
mod multipart_tests {
    use super::*;

    fn build_multipart(boundary: &str, parts: &[(&str, &str, &[u8], bool)]) -> Vec<u8> {
        let mut buf = Vec::new();
        for (key, mime, body, is_error) in parts {
            buf.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            buf.extend_from_slice(
                format!("Content-Disposition: attachment; filename=\"{key}\"\r\n").as_bytes(),
            );
            if *is_error {
                buf.extend_from_slice(b"X-Batch-Error: true\r\n");
            }
            buf.extend_from_slice(format!("Content-Type: {mime}\r\n").as_bytes());
            buf.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
            buf.extend_from_slice(b"\r\n");
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\r\n");
        }
        buf.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        buf
    }

    #[test]
    fn parses_text_and_binary_parts() {
        let boundary = "test-boundary-123";
        let body = build_multipart(
            boundary,
            &[
                ("files/readme.md", "text/markdown", b"# Hello", false),
                (
                    "files/image.png",
                    "image/png",
                    &[0x89, 0x50, 0x4E, 0x47],
                    false,
                ),
            ],
        );
        let result = parse_multipart_batch(&body, boundary);
        assert_eq!(result.objects.len(), 2);
        assert!(result.errors.is_empty());

        let md = result.objects.get("files/readme.md").unwrap();
        assert_eq!(md.bytes, b"# Hello");
        assert_eq!(md.mime_type, "text/markdown");

        let img = result.objects.get("files/image.png").unwrap();
        assert_eq!(img.bytes, &[0x89, 0x50, 0x4E, 0x47]);
        assert_eq!(img.mime_type, "image/png");
    }

    #[test]
    fn parses_error_parts() {
        let boundary = "err-boundary";
        let body = build_multipart(
            boundary,
            &[
                ("files/ok.md", "text/markdown", b"content", false),
                ("files/missing.md", "text/plain", b"Object not found", true),
            ],
        );
        let result = parse_multipart_batch(&body, boundary);
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors.get("files/missing.md").unwrap(),
            "Object not found"
        );
    }

    #[test]
    fn handles_empty_batch() {
        let boundary = "empty";
        let body = format!("--{boundary}--\r\n").into_bytes();
        let result = parse_multipart_batch(&body, boundary);
        assert!(result.objects.is_empty());
        assert!(result.errors.is_empty());
    }
}

/// No-op implementation of [`PluginStorage`] for plugins that don't need persistence.
pub struct NoopStorage;

impl PluginStorage for NoopStorage {
    fn get(&self, _key: &str) -> Option<Vec<u8>> {
        None
    }
    fn set(&self, _key: &str, _data: &[u8]) {}
    fn delete(&self, _key: &str) {}
}

/// No-op implementation of [`PluginSecretStore`] for hosts without secure storage.
pub struct NoopSecretStore;

impl PluginSecretStore for NoopSecretStore {
    fn get(&self, _key: &str) -> Option<String> {
        None
    }

    fn set(&self, _key: &str, _value: &str) {}

    fn delete(&self, _key: &str) {}
}

fn sanitize_storage_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == ':' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// File-backed [`PluginStorage`] implementation for native hosts.
pub struct FilePluginStorage {
    base_dir: PathBuf,
}

impl FilePluginStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        self.base_dir
            .join(format!("{}.bin", sanitize_storage_key(key)))
    }
}

impl PluginStorage for FilePluginStorage {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.key_to_path(key)).ok()
    }

    fn set(&self, key: &str, data: &[u8]) {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, data);
    }

    fn delete(&self, key: &str) {
        let _ = std::fs::remove_file(self.key_to_path(key));
    }
}

/// File-backed [`PluginSecretStore`] implementation for native hosts.
pub struct FilePluginSecretStore {
    base_dir: PathBuf,
}

impl FilePluginSecretStore {
    pub fn new(base_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        self.base_dir
            .join(format!("{}.secret", sanitize_storage_key(key)))
    }
}

impl PluginSecretStore for FilePluginSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.key_to_path(key)).ok()
    }

    fn set(&self, key: &str, value: &str) {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, value);
    }

    fn delete(&self, key: &str) {
        let _ = std::fs::remove_file(self.key_to_path(key));
    }
}

/// Trait for providing user-selected files to plugins.
///
/// On CLI, files come from command-line arguments (paths read into memory).
/// On browser, files come from File input elements or drag-and-drop.
/// Plugins request files by key name (e.g. "source_file", "dayone_export").
pub trait FileProvider: Send + Sync {
    /// Get file bytes by key name. Returns `None` if no file is available for that key.
    fn get_file(&self, plugin_id: &str, key: &str) -> Option<Vec<u8>>;
}

/// No-op implementation of [`FileProvider`] — always returns `None`.
pub struct NoopFileProvider;

impl FileProvider for NoopFileProvider {
    fn get_file(&self, _plugin_id: &str, _key: &str) -> Option<Vec<u8>> {
        None
    }
}

/// [`FileProvider`] backed by a pre-populated map.
///
/// Used by the CLI to pass files read from command-line arguments.
pub struct MapFileProvider {
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl MapFileProvider {
    pub fn new(files: std::collections::HashMap<String, Vec<u8>>) -> Self {
        Self { files }
    }
}

impl FileProvider for MapFileProvider {
    fn get_file(&self, _plugin_id: &str, key: &str) -> Option<Vec<u8>> {
        self.files.get(key).cloned()
    }
}

/// No-op implementation of [`EventEmitter`] for plugins that don't emit events.
pub struct NoopEventEmitter;

impl EventEmitter for NoopEventEmitter {
    fn emit(&self, _event_json: &str) {}
}

/// No-op websocket bridge for hosts that don't support plugin-managed transport.
pub struct NoopWebSocketBridge;

impl WebSocketBridge for NoopWebSocketBridge {
    fn request(&self, _request_json: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

/// No-op plugin command bridge for hosts that do not support plugin-to-plugin calls.
pub struct NoopPluginCommandBridge;

impl PluginCommandBridge for NoopPluginCommandBridge {
    fn call(
        &self,
        _caller_plugin_id: &str,
        _plugin_id: &str,
        _command: &str,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("Plugin command bridge is not available".to_string())
    }
}

/// No-op runtime context provider for hosts without runtime context wiring.
pub struct NoopRuntimeContextProvider;

impl RuntimeContextProvider for NoopRuntimeContextProvider {
    fn get_context(&self, _plugin_id: &str) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// No-op namespace provider for hosts that don't support namespace operations.
pub struct NoopNamespaceProvider;

impl NamespaceProvider for NoopNamespaceProvider {
    fn create_namespace(
        &self,
        _metadata: Option<&serde_json::Value>,
    ) -> Result<NamespaceEntry, String> {
        Err("Namespace operations are not available".to_string())
    }

    fn put_object(
        &self,
        _ns_id: &str,
        _key: &str,
        _bytes: &[u8],
        _mime_type: &str,
        _audience: Option<&str>,
    ) -> Result<(), String> {
        Err("Namespace operations are not available".to_string())
    }
    fn get_object(&self, _ns_id: &str, _key: &str) -> Result<Vec<u8>, String> {
        Err("Namespace operations are not available".to_string())
    }
    fn delete_object(&self, _ns_id: &str, _key: &str) -> Result<(), String> {
        Err("Namespace operations are not available".to_string())
    }
    fn list_objects(
        &self,
        _ns_id: &str,
        _prefix: Option<&str>,
        _limit: Option<u32>,
        _offset: Option<u32>,
    ) -> Result<Vec<NamespaceObjectMeta>, String> {
        Err("Namespace operations are not available".to_string())
    }
    fn sync_audience(
        &self,
        _ns_id: &str,
        _audience: &str,
        _gates: &serde_json::Value,
    ) -> Result<(), String> {
        Err("Namespace operations are not available".to_string())
    }
    fn list_audiences(&self, _ns_id: &str) -> Result<Vec<String>, String> {
        Err("Namespace operations are not available".to_string())
    }
    fn delete_audience(&self, _ns_id: &str, _audience: &str) -> Result<(), String> {
        Err("Namespace operations are not available".to_string())
    }

    fn get_objects_batch(&self, _ns_id: &str, _keys: &[String]) -> Result<BatchGetResult, String> {
        Err("Namespace operations are not available".to_string())
    }

    fn list_namespaces(&self) -> Result<Vec<NamespaceEntry>, String> {
        Err("Namespace operations are not available".to_string())
    }
}

/// Trait for checking plugin permissions before allowing host function calls.
///
/// Implementations may check static config, prompt the user, or consult
/// a session-level cache.
pub trait PermissionChecker: Send + Sync {
    /// Check if a plugin has permission for an action.
    ///
    /// Returns `Ok(())` if allowed, `Err(message)` if denied.
    /// The `target` is context-dependent: file path, URL, command name, etc.
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String>;

    /// Return the configured per-plugin storage quota in bytes.
    ///
    /// `None` means "use the host's default quota". The host will cap any
    /// returned value at [`MAX_STORAGE_QUOTA_BYTES`] regardless, so a
    /// misconfigured workspace can't exceed the ceiling.
    fn storage_quota_bytes(&self, plugin_id: &str) -> Option<u64> {
        let _ = plugin_id;
        None
    }
}

/// Context shared with host functions via Extism's `UserData` mechanism.
///
/// Provides guest plugins with controlled access to the workspace filesystem,
/// persistent storage, and event dispatch.
pub struct HostContext {
    /// Type-erased async filesystem for workspace file access.
    pub fs: Arc<dyn AsyncFileSystem>,
    /// Persistent storage for plugin state (CRDT snapshots, etc.).
    pub storage: Arc<dyn PluginStorage>,
    /// Persistent storage for plugin secrets (tokens, API keys).
    pub secret_store: Arc<dyn PluginSecretStore>,
    /// Event emitter for sync events.
    pub event_emitter: Arc<dyn EventEmitter>,
    /// Which plugin this context belongs to.
    pub plugin_id: String,
    /// Whether the plugin ID has been set from a guest manifest and should not be overwritten.
    pub plugin_id_locked: bool,
    /// Permission checker (None = deny all).
    pub permission_checker: Option<Arc<dyn PermissionChecker>>,
    /// Provider of user-selected files (e.g. from CLI args or browser file picker).
    pub file_provider: Arc<dyn FileProvider>,
    /// WebSocket bridge for plugin-managed sync transport.
    pub ws_bridge: Arc<dyn WebSocketBridge>,
    /// Host-mediated plugin-to-plugin command bridge.
    pub plugin_command_bridge: Arc<dyn PluginCommandBridge>,
    /// Provider of generic runtime context for the caller plugin.
    pub runtime_context_provider: Arc<dyn RuntimeContextProvider>,
    /// Provider of namespace object operations (upload, delete, list, sync).
    pub namespace_provider: Arc<dyn NamespaceProvider>,
    /// Current cross-plugin command call depth (prevents infinite recursion).
    pub plugin_command_depth: u32,
    /// Maximum storage bytes per plugin (0 = unlimited). Default: 1 MiB.
    pub storage_quota_bytes: u64,
}

/// Default plugin storage quota: 1 MiB.
///
/// Used when the workspace frontmatter doesn't specify a `quota_bytes` for
/// the plugin's `plugin_storage` permission rule.
pub const DEFAULT_STORAGE_QUOTA_BYTES: u64 = 1024 * 1024;

/// Hard ceiling on per-plugin storage quota: 1 GiB.
///
/// The host caps the effective quota at this value regardless of what the
/// plugin requests or what the user approves in frontmatter. Prevents a
/// plugin from claiming unlimited disk via the permission system.
pub const MAX_STORAGE_QUOTA_BYTES: u64 = 1024 * 1024 * 1024;

impl HostContext {
    /// Create a context with just a filesystem (backwards compatible).
    pub fn with_fs(fs: Arc<dyn AsyncFileSystem>) -> Self {
        Self {
            fs,
            storage: Arc::new(NoopStorage),
            secret_store: Arc::new(NoopSecretStore),
            event_emitter: Arc::new(NoopEventEmitter),
            plugin_id: String::new(),
            plugin_id_locked: false,
            permission_checker: Some(Arc::new(DenyAllPermissionChecker)),
            file_provider: Arc::new(NoopFileProvider),
            ws_bridge: Arc::new(NoopWebSocketBridge),
            plugin_command_bridge: Arc::new(NoopPluginCommandBridge),
            runtime_context_provider: Arc::new(NoopRuntimeContextProvider),
            namespace_provider: Arc::new(NoopNamespaceProvider),
            plugin_command_depth: 0,
            storage_quota_bytes: DEFAULT_STORAGE_QUOTA_BYTES,
        }
    }

    /// Check a permission, returning an Extism error if denied.
    fn check_perm(&self, perm: PermissionType, target: &str) -> Result<(), ExtismError> {
        if let Some(checker) = &self.permission_checker {
            checker
                .check_permission(&self.plugin_id, perm, target)
                .map_err(|msg| ExtismError::msg(msg))
        } else {
            Err(ExtismError::msg(
                "Permission checker is not configured for this plugin host context",
            ))
        }
    }

    /// Validate HTTP header names and values to prevent header injection.
    ///
    /// Rejects headers containing newlines, null bytes, or carriage returns.
    fn validate_http_headers(
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<(), ExtismError> {
        for (name, value) in headers {
            if name.contains('\n')
                || name.contains('\r')
                || name.contains('\0')
                || value.contains('\n')
                || value.contains('\r')
                || value.contains('\0')
            {
                return Err(ExtismError::msg(format!(
                    "Invalid HTTP header: name or value contains forbidden characters (header: '{name}')"
                )));
            }
        }
        Ok(())
    }

    /// Validate and canonicalize a file path to prevent directory traversal.
    ///
    /// Rejects paths containing `..` components that could escape the workspace.
    /// Returns the cleaned path string suitable for passing to the filesystem.
    fn validate_file_path(path: &str) -> Result<String, ExtismError> {
        let normalized = path.replace('\\', "/");
        for component in normalized.split('/') {
            if component == ".." {
                return Err(ExtismError::msg(format!(
                    "Path traversal not allowed: '{path}'"
                )));
            }
        }
        Ok(path.to_string())
    }

    fn storage_key(&self, key: &str) -> String {
        if self.plugin_id.is_empty() {
            key.to_string()
        } else {
            format!("{}:{}", self.plugin_id, key)
        }
    }

    fn secret_key(&self, key: &str) -> String {
        self.storage_key(key)
    }
}

// SAFETY: HostContext only contains Arc<dyn Trait> values which require
// Send + Sync on native targets.
unsafe impl Send for HostContext {}
unsafe impl Sync for HostContext {}

/// Register all host functions on an Extism `PluginBuilder`.
///
/// The builder is consumed and returned with host functions attached.
pub fn register_host_functions(
    builder: extism::PluginBuilder<'_>,
    user_data: UserData<HostContext>,
) -> extism::PluginBuilder<'_> {
    builder
        .with_function(
            "host_log",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_log,
        )
        .with_function(
            "host_read_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_read_file,
        )
        .with_function(
            "host_read_binary",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_read_binary,
        )
        .with_function(
            "host_list_files",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_list_files,
        )
        .with_function(
            "host_list_dir",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_list_dir,
        )
        .with_function(
            "host_workspace_file_set",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_workspace_file_set,
        )
        .with_function(
            "host_file_exists",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_file_exists,
        )
        .with_function(
            "host_file_metadata",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_file_metadata,
        )
        .with_function(
            "host_write_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_write_file,
        )
        .with_function(
            "host_delete_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_delete_file,
        )
        .with_function(
            "host_write_binary",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_write_binary,
        )
        .with_function(
            "host_emit_event",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_emit_event,
        )
        .with_function(
            "host_storage_get",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_storage_get,
        )
        .with_function(
            "host_storage_set",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_storage_set,
        )
        .with_function(
            "host_secret_get",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_secret_get,
        )
        .with_function(
            "host_secret_set",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_secret_set,
        )
        .with_function(
            "host_secret_delete",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_secret_delete,
        )
        .with_function(
            "host_get_timestamp",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_get_timestamp,
        )
        .with_function(
            "host_get_now",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_get_now,
        )
        .with_function(
            "host_http_request",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_http_request,
        )
        .with_function(
            "host_run_wasi_module",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_run_wasi_module,
        )
        .with_function(
            "host_request_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_request_file,
        )
        .with_function(
            "host_plugin_command",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_plugin_command,
        )
        .with_function(
            "host_get_runtime_context",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_get_runtime_context,
        )
        .with_function(
            "host_namespace_put_object",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_put_object,
        )
        .with_function(
            "host_namespace_delete_object",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_delete_object,
        )
        .with_function(
            "host_namespace_get_object",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_get_object,
        )
        .with_function(
            "host_namespace_get_objects_batch",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_get_objects_batch,
        )
        .with_function(
            "host_namespace_list_objects",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_list_objects,
        )
        .with_function(
            "host_namespace_list",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_list,
        )
        .with_function(
            "host_namespace_create",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_create,
        )
        .with_function(
            "host_namespace_sync_audience",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_sync_audience,
        )
        .with_function(
            "host_namespace_list_audiences",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_list_audiences,
        )
        .with_function(
            "host_namespace_delete_audience",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_namespace_delete_audience,
        )
        .with_function(
            "host_ws_request",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_ws_request,
        )
        .with_function(
            "host_hash_file",
            [ValType::I64],
            [ValType::I64],
            user_data.clone(),
            host_hash_file,
        )
        .with_function(
            "host_proxy_request",
            [ValType::I64],
            [ValType::I64],
            user_data,
            host_proxy_request,
        )
}

/// Host function: `host_log(input: {level, message}) -> ""`
///
/// Logs a message via the `log` crate at the specified level.
fn host_log(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct LogInput {
        level: String,
        message: String,
    }

    let parsed: LogInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_log: invalid input: {e}")))?;

    match parsed.level.as_str() {
        "error" => log::error!("[extism-plugin] {}", parsed.message),
        "warn" => log::warn!("[extism-plugin] {}", parsed.message),
        "info" => log::info!("[extism-plugin] {}", parsed.message),
        "debug" => log::debug!("[extism-plugin] {}", parsed.message),
        _ => log::trace!("[extism-plugin] {}", parsed.message),
    }

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_read_file(input: {path}) -> file content string or {"error": "..."}`
///
/// Reads a workspace file and returns its content.
/// Returns a JSON error object instead of trapping on I/O or permission errors,
/// so the guest can handle missing files gracefully via `.ok()`.
fn host_read_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ReadInput {
        path: String,
    }

    let parsed: ReadInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_read_file: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_read_file: lock: {e}")))?;

    if let Err(e) = ctx.check_perm(PermissionType::ReadFiles, &path) {
        let err = serde_json::json!({ "error": e.to_string() }).to_string();
        plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        return Ok(());
    }

    match futures_lite::future::block_on(ctx.fs.read_to_string(Path::new(&path))) {
        Ok(content) => {
            plugin.memory_set_val(&mut outputs[0], content.as_str())?;
        }
        Err(e) => {
            let err = serde_json::json!({ "error": format!("host_read_file: {e}") }).to_string();
            plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        }
    }
    Ok(())
}

/// Host function: `host_read_binary(input: {path}) -> {data: base64}`
///
/// Reads a workspace file as raw bytes.
fn host_read_binary(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ReadInput {
        path: String,
    }

    let parsed: ReadInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_read_binary: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_read_binary: lock: {e}")))?;

    if let Err(e) = ctx.check_perm(PermissionType::ReadFiles, &path) {
        let err = serde_json::json!({ "error": e.to_string() }).to_string();
        plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        return Ok(());
    }

    match futures_lite::future::block_on(ctx.fs.read_binary(Path::new(&path))) {
        Ok(bytes) => {
            let json = serde_json::json!({
                "data": base64::engine::general_purpose::STANDARD.encode(&bytes)
            })
            .to_string();
            plugin.memory_set_val(&mut outputs[0], json.as_str())?;
        }
        Err(e) => {
            let err = serde_json::json!({ "error": format!("host_read_binary: {e}") }).to_string();
            plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        }
    }
    Ok(())
}

/// Host function: `host_list_dir(input: {path}) -> string[] JSON`
///
/// Lists direct children of a directory (non-recursive, single level).
/// Returns paths as strings. This is much cheaper than `host_list_files`
/// for large workspaces because it never descends into subdirectories.
fn host_list_dir(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ListDirInput {
        path: String,
    }

    let parsed: ListDirInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_list_dir: invalid input: {e}")))?;
    let dir_path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_list_dir: lock: {e}")))?;
    if let Err(e) = ctx.check_perm(PermissionType::ReadFiles, &dir_path) {
        let err = serde_json::json!({ "error": e.to_string() }).to_string();
        plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        return Ok(());
    }
    let files = match futures_lite::future::block_on(ctx.fs.list_files(Path::new(&dir_path))) {
        Ok(files) => files,
        Err(e) => {
            let err = serde_json::json!({ "error": format!("host_list_dir: {e}") }).to_string();
            plugin.memory_set_val(&mut outputs[0], err.as_str())?;
            return Ok(());
        }
    };

    let file_strings: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let json = serde_json::to_string(&file_strings)
        .map_err(|e| ExtismError::msg(format!("host_list_dir: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_list_files(input: {prefix}) -> string[] JSON`
///
/// Lists files under a given prefix in the workspace.
fn host_list_files(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ListInput {
        prefix: String,
    }

    let parsed: ListInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_list_files: invalid input: {e}")))?;
    let prefix = HostContext::validate_file_path(&parsed.prefix)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_list_files: lock: {e}")))?;
    if let Err(e) = ctx.check_perm(PermissionType::ReadFiles, &prefix) {
        let err = serde_json::json!({ "error": e.to_string() }).to_string();
        plugin.memory_set_val(&mut outputs[0], err.as_str())?;
        return Ok(());
    }
    let files =
        match futures_lite::future::block_on(ctx.fs.list_all_files_recursive(Path::new(&prefix))) {
            Ok(files) => files,
            Err(e) => {
                let err =
                    serde_json::json!({ "error": format!("host_list_files: {e}") }).to_string();
                plugin.memory_set_val(&mut outputs[0], err.as_str())?;
                return Ok(());
            }
        };

    let file_strings: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let json = serde_json::to_string(&file_strings)
        .map_err(|e| ExtismError::msg(format!("host_list_files: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_workspace_file_set({}) -> string[] JSON`
///
/// Returns the canonical workspace-relative file set for the current
/// workspace, including reachable markdown files and declared attachments.
fn host_workspace_file_set(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    // Inner function returns Result so we can use `?` for control flow, then
    // the outer function converts errors to output strings instead of WASM
    // traps so the guest can handle them gracefully.
    fn inner(user_data: &UserData<HostContext>) -> Result<Vec<String>, String> {
        let ctx = user_data
            .get()
            .map_err(|e| format!("host_workspace_file_set: user_data: {e}"))?;
        let ctx = ctx
            .lock()
            .map_err(|e| format!("host_workspace_file_set: lock: {e}"))?;
        let runtime = ctx.runtime_context_provider.get_context(&ctx.plugin_id);
        let workspace_path = runtime
            .get("current_workspace")
            .and_then(|value| value.as_object())
            .and_then(|workspace| workspace.get("path"))
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or("host_workspace_file_set: missing current_workspace.path")?;

        ctx.check_perm(PermissionType::ReadFiles, workspace_path)
            .map_err(|e| e.to_string())?;

        let workspace = diaryx_core::workspace::Workspace::new(ctx.fs.as_ref());
        let workspace_path = Path::new(workspace_path);
        let root_index = if workspace_path
            .extension()
            .is_some_and(|extension| extension == "md")
        {
            workspace_path.to_path_buf()
        } else {
            futures_lite::future::block_on(workspace.find_root_index_in_dir(workspace_path))
                .map_err(|e| format!("host_workspace_file_set: {e}"))?
                .ok_or("host_workspace_file_set: workspace root index not found")?
        };

        futures_lite::future::block_on(workspace.collect_workspace_file_set(&root_index))
            .map_err(|e| format!("host_workspace_file_set: {e}"))
    }

    match inner(&user_data) {
        Ok(files) => {
            let json = serde_json::to_string(&files).map_err(|e| {
                ExtismError::msg(format!("host_workspace_file_set: serialize: {e}"))
            })?;
            plugin.memory_set_val(&mut outputs[0], json.as_str())?;
        }
        Err(msg) => {
            // Return the error as the output string. The guest SDK will fail
            // to parse it as JSON and surface it as Result::Err.
            plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        }
    }
    Ok(())
}

/// Host function: `host_file_exists(input: {path}) -> bool JSON`
///
/// Checks if a file exists in the workspace.
fn host_file_exists(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ExistsInput {
        path: String,
    }

    let parsed: ExistsInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_file_exists: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_file_exists: lock: {e}")))?;
    // Permission errors return `false` (effectively "not visible") rather than
    // trapping, mirroring the existing graceful pattern in host_hash_file.
    if ctx.check_perm(PermissionType::ReadFiles, &path).is_err() {
        plugin.memory_set_val(&mut outputs[0], "false")?;
        return Ok(());
    }
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&path)));

    let json = serde_json::to_string(&exists)
        .map_err(|e| ExtismError::msg(format!("host_file_exists: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_file_metadata(input: {path}) -> {exists, size_bytes?, modified_at_ms?}`
///
/// Returns lightweight metadata for a workspace file without reading its bytes.
fn host_file_metadata(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct MetadataInput {
        path: String,
    }

    let parsed: MetadataInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_file_metadata: invalid input: {e}")))?;
    let validated_path = HostContext::validate_file_path(&parsed.path)?;

    let not_found = serde_json::json!({
        "exists": false,
        "size_bytes": serde_json::Value::Null,
        "modified_at_ms": serde_json::Value::Null,
    })
    .to_string();

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_file_metadata: lock: {e}")))?;
    // Permission or filesystem errors return "not found" rather than trapping.
    if ctx
        .check_perm(PermissionType::ReadFiles, &validated_path)
        .is_err()
    {
        plugin.memory_set_val(&mut outputs[0], not_found.as_str())?;
        return Ok(());
    }
    let path = Path::new(&validated_path);
    let exists = futures_lite::future::block_on(ctx.fs.exists(path));
    let json = if exists {
        let size_bytes = futures_lite::future::block_on(ctx.fs.get_file_size(path));
        let modified_at_ms = futures_lite::future::block_on(ctx.fs.get_modified_time(path));
        serde_json::json!({
            "exists": true,
            "size_bytes": size_bytes,
            "modified_at_ms": modified_at_ms,
        })
        .to_string()
    } else {
        not_found
    };

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_write_file(input: {path, content}) -> ""`
///
/// Writes a text file to the workspace.
fn host_write_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct WriteInput {
        path: String,
        content: String,
    }

    let parsed: WriteInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_write_file: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_write_file: lock: {e}")))?;
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&path)));
    let perm = if exists {
        PermissionType::EditFiles
    } else {
        PermissionType::CreateFiles
    };
    if let Err(e) = ctx.check_perm(perm, &path) {
        plugin.memory_set_val(&mut outputs[0], e.to_string().as_str())?;
        return Ok(());
    }
    // Return filesystem errors as a string rather than propagating them as
    // ExtismError.  An ExtismError causes a WASM trap that aborts the entire
    // guest call — the guest code never gets a chance to handle it.  By
    // returning the error message in the output the guest SDK can surface it
    // as a normal `Result::Err` that callers can recover from.
    if let Err(e) =
        futures_lite::future::block_on(ctx.fs.write_file(Path::new(&path), &parsed.content))
    {
        let msg = format!("host_write_file: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_delete_file(input: {path}) -> "" | error`
///
/// Deletes a file from the workspace.
/// Returns an empty string on success, or an error message on failure.
fn host_delete_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct DeleteInput {
        path: String,
    }

    let parsed: DeleteInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_delete_file: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_delete_file: lock: {e}")))?;
    if let Err(e) = ctx.check_perm(PermissionType::DeleteFiles, &path) {
        plugin.memory_set_val(&mut outputs[0], e.to_string().as_str())?;
        return Ok(());
    }
    // Return filesystem errors as a recoverable string — see host_write_file
    // comment for rationale.
    if let Err(e) = futures_lite::future::block_on(ctx.fs.delete_file(Path::new(&path))) {
        let msg = format!("host_delete_file: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_write_binary(input: {path, content}) -> "" | error`
///
/// Writes binary content (base64-encoded) to a file.
/// Returns an empty string on success, or an error message on failure.
fn host_write_binary(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct WriteBinaryInput {
        path: String,
        content: String, // base64-encoded
    }

    let parsed: WriteBinaryInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_write_binary: invalid input: {e}")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed.content)
        .map_err(|e| ExtismError::msg(format!("host_write_binary: base64 decode: {e}")))?;

    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_write_binary: lock: {e}")))?;
    let exists = futures_lite::future::block_on(ctx.fs.exists(Path::new(&path)));
    let perm = if exists {
        PermissionType::EditFiles
    } else {
        PermissionType::CreateFiles
    };
    if let Err(e) = ctx.check_perm(perm, &path) {
        plugin.memory_set_val(&mut outputs[0], e.to_string().as_str())?;
        return Ok(());
    }
    // Return filesystem errors as a recoverable string — see host_write_file
    // comment for rationale.
    if let Err(e) = futures_lite::future::block_on(ctx.fs.write_binary(Path::new(&path), &bytes)) {
        let msg = format!("host_write_binary: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_emit_event(input: event_json) -> ""`
///
/// Emits a sync event to the host application.
fn host_emit_event(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let event_json: String = plugin.memory_get_val(&inputs[0])?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_emit_event: lock: {e}")))?;
    ctx.event_emitter.emit(&event_json);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_storage_get(input: {key}) -> {data: base64} or ""`
///
/// Loads persisted state by key.
fn host_storage_get(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct StorageGetInput {
        key: String,
    }

    let parsed: StorageGetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_storage_get: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_storage_get: lock: {e}")))?;
    // Permission errors return empty (key not found) rather than trapping.
    if ctx
        .check_perm(PermissionType::PluginStorage, &parsed.key)
        .is_err()
    {
        plugin.memory_set_val(&mut outputs[0], "")?;
        return Ok(());
    }
    let storage_key = ctx.storage_key(&parsed.key);

    let result = match ctx.storage.get(&storage_key) {
        Some(data) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            serde_json::json!({ "data": encoded }).to_string()
        }
        None => String::new(),
    };

    plugin.memory_set_val(&mut outputs[0], result.as_str())?;
    Ok(())
}

/// Host function: `host_storage_set(input: {key, data}) -> ""`
///
/// Persists state by key (data is base64-encoded).
fn host_storage_set(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct StorageSetInput {
        key: String,
        data: String, // base64-encoded
    }

    let parsed: StorageSetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_storage_set: invalid input: {e}")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed.data)
        .map_err(|e| ExtismError::msg(format!("host_storage_set: base64 decode: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_storage_set: lock: {e}")))?;
    // Permission errors return an error string rather than trapping.
    if let Err(e) = ctx.check_perm(PermissionType::PluginStorage, &parsed.key) {
        let msg = format!("host_storage_set: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }
    // Resolve effective quota: prefer what the permission checker says (which
    // reads the user-approved value from workspace frontmatter), fall back to
    // the static per-context default. Cap at the hard ceiling so an
    // overzealous frontmatter value can't exceed it.
    let effective_quota = ctx
        .permission_checker
        .as_ref()
        .and_then(|c| c.storage_quota_bytes(&ctx.plugin_id))
        .unwrap_or(ctx.storage_quota_bytes)
        .min(MAX_STORAGE_QUOTA_BYTES);
    if effective_quota > 0 && bytes.len() as u64 > effective_quota {
        let msg = format!(
            "host_storage_set: data size ({} bytes) exceeds plugin storage quota ({} bytes)",
            bytes.len(),
            effective_quota
        );
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }
    let storage_key = ctx.storage_key(&parsed.key);
    ctx.storage.set(&storage_key, &bytes);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_secret_get(input: {key}) -> {value: string} or ""`
///
/// Loads a secret value by key.
fn host_secret_get(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct SecretGetInput {
        key: String,
    }

    let parsed: SecretGetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_secret_get: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_secret_get: lock: {e}")))?;
    // Permission errors return empty (key not found) rather than trapping.
    if ctx
        .check_perm(PermissionType::PluginStorage, &parsed.key)
        .is_err()
    {
        plugin.memory_set_val(&mut outputs[0], "")?;
        return Ok(());
    }
    let secret_key = ctx.secret_key(&parsed.key);

    let result = match ctx.secret_store.get(&secret_key) {
        Some(value) => serde_json::json!({ "value": value }).to_string(),
        None => String::new(),
    };

    plugin.memory_set_val(&mut outputs[0], result.as_str())?;
    Ok(())
}

/// Host function: `host_secret_set(input: {key, value}) -> ""`
///
/// Persists a secret value by key.
fn host_secret_set(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct SecretSetInput {
        key: String,
        value: String,
    }

    let parsed: SecretSetInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_secret_set: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_secret_set: lock: {e}")))?;
    // Permission errors return an error string rather than trapping.
    if let Err(e) = ctx.check_perm(PermissionType::PluginStorage, &parsed.key) {
        let msg = format!("host_secret_set: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }
    let secret_key = ctx.secret_key(&parsed.key);
    ctx.secret_store.set(&secret_key, &parsed.value);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_secret_delete(input: {key}) -> ""`
///
/// Deletes a secret value by key.
fn host_secret_delete(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct SecretDeleteInput {
        key: String,
    }

    let parsed: SecretDeleteInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_secret_delete: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_secret_delete: lock: {e}")))?;
    // Permission errors return an error string rather than trapping.
    if let Err(e) = ctx.check_perm(PermissionType::PluginStorage, &parsed.key) {
        let msg = format!("host_secret_delete: {e}");
        plugin.memory_set_val(&mut outputs[0], msg.as_str())?;
        return Ok(());
    }
    let secret_key = ctx.secret_key(&parsed.key);
    ctx.secret_store.delete(&secret_key);

    plugin.memory_set_val(&mut outputs[0], "")?;
    Ok(())
}

/// Host function: `host_run_wasi_module(input: WasiRunRequest) -> WasiRunResult`
///
/// Runs a WASI module stored in plugin storage. The guest provides a storage
/// key, CLI arguments, optional stdin, virtual filesystem files, and a list
/// of output files to capture. Only available when the `wasi-runner` feature
/// is enabled.
#[cfg(feature = "wasi-runner")]
fn host_run_wasi_module(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine;

    /// Build a `WasiRunResult`-shaped error envelope so guests can recover
    /// gracefully via the SDK's `wasi::run` parser instead of trapping.
    fn err_envelope(msg: &str) -> String {
        serde_json::json!({
            "exit_code": -1,
            "stdout": "",
            "stderr": msg,
            "files": serde_json::Value::Null,
            "error": msg,
        })
        .to_string()
    }

    let input: String = plugin.memory_get_val(&inputs[0])?;
    let request: crate::wasi_runner::WasiRunRequest = match serde_json::from_str(&input) {
        Ok(req) => req,
        Err(e) => {
            let msg = format!("host_run_wasi_module: invalid input: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };

    // Load the WASM module bytes from plugin storage
    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_run_wasi_module: lock: {e}")))?;
    if let Err(e) = ctx.check_perm(PermissionType::PluginStorage, &request.module_key) {
        let msg = format!("host_run_wasi_module: {e}");
        plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
        return Ok(());
    }
    let storage_key = ctx.storage_key(&request.module_key);
    let wasm_bytes = match ctx.storage.get(&storage_key) {
        Some(bytes) => bytes,
        None => {
            let msg = format!(
                "host_run_wasi_module: module not found in storage: {}",
                request.module_key
            );
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };
    drop(ctx);

    // Decode input files from base64
    let decoded_files = if let Some(ref files) = request.files {
        let mut map = std::collections::HashMap::new();
        for (path, b64) in files {
            match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(data) => {
                    map.insert(path.clone(), data);
                }
                Err(e) => {
                    let msg = format!("host_run_wasi_module: base64 decode for {path}: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            }
        }
        Some(map)
    } else {
        None
    };

    // Decode stdin from base64
    let stdin_bytes = if let Some(ref b64) = request.stdin {
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(bytes) => Some(bytes),
            Err(e) => {
                let msg = format!("host_run_wasi_module: stdin base64 decode: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    } else {
        None
    };

    // Run the module
    let result = match crate::wasi_runner::run_wasi_module(
        &wasm_bytes,
        &request.args,
        stdin_bytes.as_deref(),
        decoded_files.as_ref(),
        request.output_files.as_deref(),
    ) {
        Ok(result) => result,
        Err(e) => {
            let msg = format!("host_run_wasi_module: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };

    let json = serde_json::to_string(&result)
        .map_err(|e| ExtismError::msg(format!("host_run_wasi_module: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Stub for `host_run_wasi_module` when the `wasi-runner` feature is not enabled.
#[cfg(not(feature = "wasi-runner"))]
fn host_run_wasi_module(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let error = serde_json::json!({
        "exit_code": -1,
        "stdout": "",
        "stderr": "host_run_wasi_module: wasi-runner feature not enabled"
    });
    plugin.memory_set_val(&mut outputs[0], error.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_get_timestamp(input: "") -> timestamp_ms string`
///
/// Returns the current timestamp in milliseconds since epoch.
fn host_get_timestamp(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    plugin.memory_set_val(&mut outputs[0], now.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_get_now(input: "") -> local RFC 3339 timestamp string`
fn host_get_now(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let now = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    plugin.memory_set_val(&mut outputs[0], now.as_str())?;
    Ok(())
}

/// Host function: `host_request_file(input: {key}) -> raw bytes or empty`
///
/// Requests a user-provided file by key name. The host decides where the
/// file comes from (CLI: read from path in command args; browser: File picker).
/// Returns the raw file bytes, or an empty result if unavailable.
fn host_request_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct RequestFileInput {
        key: String,
    }

    let parsed: RequestFileInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_request_file: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_request_file: lock: {e}")))?;

    let result = ctx
        .file_provider
        .get_file(&ctx.plugin_id, &parsed.key)
        .unwrap_or_default();

    plugin.memory_set_val(&mut outputs[0], result.as_slice())?;
    Ok(())
}

/// Host function: `host_plugin_command(input: {plugin_id, command, params}) -> {success, data?, error?}`
///
/// Executes a command on another loaded plugin through the host bridge.
fn host_plugin_command(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    #[derive(serde::Deserialize)]
    struct PluginCommandInput {
        plugin_id: String,
        command: String,
        #[serde(default)]
        params: serde_json::Value,
    }

    let input: String = plugin.memory_get_val(&inputs[0])?;
    let parsed: PluginCommandInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_plugin_command: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_plugin_command: lock: {e}")))?;

    const MAX_PLUGIN_COMMAND_DEPTH: u32 = 8;

    let response = if ctx.plugin_command_depth >= MAX_PLUGIN_COMMAND_DEPTH {
        serde_json::json!({
            "success": false,
            "error": format!(
                "Cross-plugin command call depth limit exceeded (max {MAX_PLUGIN_COMMAND_DEPTH})"
            ),
        })
    } else if parsed.plugin_id.trim().is_empty() || parsed.command.trim().is_empty() {
        serde_json::json!({
            "success": false,
            "error": "plugin_id and command are required",
        })
    } else if parsed.plugin_id == ctx.plugin_id {
        serde_json::json!({
            "success": false,
            "error": "Plugins cannot call their own commands via host_plugin_command",
        })
    } else {
        let permission_target = format!("{}:{}", parsed.plugin_id, parsed.command);
        match ctx.check_perm(PermissionType::ExecuteCommands, &permission_target) {
            Ok(()) => match ctx.plugin_command_bridge.call(
                &ctx.plugin_id,
                &parsed.plugin_id,
                &parsed.command,
                parsed.params,
            ) {
                Ok(data) => serde_json::json!({
                    "success": true,
                    "data": data,
                }),
                Err(error) => serde_json::json!({
                    "success": false,
                    "error": error,
                }),
            },
            Err(error) => serde_json::json!({
                "success": false,
                "error": error.to_string(),
            }),
        }
    };

    let json = serde_json::to_string(&response)
        .map_err(|e| ExtismError::msg(format!("host_plugin_command: serialize: {e}")))?;
    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_get_runtime_context(input: "") -> json`
///
/// Returns generic host runtime context for the caller plugin.
fn host_get_runtime_context(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_get_runtime_context: lock: {e}")))?;
    let json = serde_json::to_string(&ctx.runtime_context_provider.get_context(&ctx.plugin_id))
        .map_err(|e| ExtismError::msg(format!("host_get_runtime_context: serialize: {e}")))?;
    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_ws_request(input: json) -> string`
///
/// Forward-compatible bridge for plugin-managed websocket ownership.
/// The concrete host bridge owns the socket lifecycle and maps these
/// requests to runtime-specific websocket operations.
fn host_ws_request(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;
    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_ws_request: lock: {e}")))?;
    // Bridge errors return as a JSON envelope so the guest can recover
    // gracefully instead of trapping the entire `handle_command` call.
    let result = match ctx.ws_bridge.request(&input) {
        Ok(s) => s,
        Err(e) => serde_json::json!({
            "ok": false,
            "error": format!("host_ws_request: {e}"),
        })
        .to_string(),
    };
    plugin.memory_set_val(&mut outputs[0], result.as_str())?;
    Ok(())
}

/// Host function: `host_hash_file(input: {path}) -> {hash: "hex..."}`
///
/// Computes the SHA-256 hash of a workspace file and returns the hex digest.
/// Returns an empty string if the file does not exist.
fn host_hash_file(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct HashInput {
        path: String,
    }

    let parsed: HashInput = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_hash_file: invalid input: {e}")))?;
    let path = HostContext::validate_file_path(&parsed.path)?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_hash_file: lock: {e}")))?;
    // Permission errors return empty (same as file-not-found) rather than trapping.
    if ctx.check_perm(PermissionType::ReadFiles, &path).is_err() {
        plugin.memory_set_val(&mut outputs[0], "")?;
        return Ok(());
    }

    let hash = match futures_lite::future::block_on(ctx.fs.hash_file(Path::new(&path))) {
        Ok(hash) => hash,
        Err(_) => {
            plugin.memory_set_val(&mut outputs[0], "")?;
            return Ok(());
        }
    };

    let json = serde_json::json!({ "hash": hash }).to_string();
    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_proxy_request(input: {proxy_id, path, method, headers, body?}) -> {status, headers, body}`
///
/// Routes a request through the server's generic proxy service.
/// The host resolves the server URL and auth token from the runtime context,
/// then makes a request to `POST {server}/api/proxy/{proxy_id}/{path}`.
#[cfg(feature = "http")]
fn host_proxy_request(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct ProxyInput {
        proxy_id: String,
        #[serde(default)]
        path: String,
        #[serde(default = "default_method")]
        method: String,
        #[serde(default)]
        headers: std::collections::HashMap<String, String>,
        body: Option<String>,
    }

    fn default_method() -> String {
        "POST".to_string()
    }

    #[derive(serde::Serialize)]
    struct ProxyOutput {
        status: u16,
        headers: std::collections::HashMap<String, String>,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body_base64: Option<String>,
    }

    /// Build an `HttpResponse`-shaped error envelope so guests can recover via
    /// the SDK's `proxy::request` parser instead of trapping.
    fn err_envelope(msg: &str) -> String {
        serde_json::json!({
            "status": 0,
            "headers": {},
            "body": msg,
            "error": msg,
        })
        .to_string()
    }

    let parsed: ProxyInput = match serde_json::from_str(&input) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("host_proxy_request: invalid input: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };

    if let Err(e) = HostContext::validate_http_headers(&parsed.headers) {
        let msg = format!("host_proxy_request: {e}");
        plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
        return Ok(());
    }

    // Resolve server URL and auth token from runtime context
    let (server_url, auth_token) = {
        let ctx = user_data.get()?;
        let ctx = ctx
            .lock()
            .map_err(|e| ExtismError::msg(format!("host_proxy_request: lock: {e}")))?;

        let runtime_json = ctx.runtime_context_provider.get_context(&ctx.plugin_id);
        let server_url = match runtime_json
            .get("server_url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string())
        {
            Some(url) => url,
            None => {
                let msg = "host_proxy_request: server_url not available in runtime context";
                plugin.memory_set_val(&mut outputs[0], err_envelope(msg).as_str())?;
                return Ok(());
            }
        };
        let auth_token = runtime_json
            .get("auth_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        (server_url, auth_token)
    };

    // Build proxy URL
    let proxy_url = if parsed.path.is_empty() {
        format!("{}/api/proxy/{}", server_url, parsed.proxy_id)
    } else {
        format!(
            "{}/api/proxy/{}/{}",
            server_url,
            parsed.proxy_id,
            parsed.path.trim_start_matches('/')
        )
    };

    // Build request
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into();

    let mut request_builder = ureq::http::Request::builder()
        .method(parsed.method.as_str())
        .uri(proxy_url.as_str())
        .header("Content-Type", "application/json");

    if let Some(ref token) = auth_token {
        request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
    }

    for (key, value) in &parsed.headers {
        request_builder = request_builder.header(key, value);
    }

    let response = if let Some(body) = &parsed.body {
        match request_builder.body(body.clone()) {
            Ok(request) => match agent.run(request) {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("host_proxy_request: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            },
            Err(e) => {
                let msg = format!("host_proxy_request: build request: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    } else {
        match request_builder.body(()) {
            Ok(request) => match agent.run(request) {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("host_proxy_request: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            },
            Err(e) => {
                let msg = format!("host_proxy_request: build request: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    };

    let status = response.status().as_u16();
    let mut resp_headers = std::collections::HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            resp_headers.insert(name.to_string(), v.to_string());
        }
    }
    let mut response = response;
    let body_bytes = match response
        .body_mut()
        .with_config()
        .limit(128 * 1024 * 1024)
        .read_to_vec()
    {
        Ok(bytes) => bytes,
        Err(e) => {
            let msg = format!("host_proxy_request: read body: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };
    let body = String::from_utf8_lossy(&body_bytes).to_string();
    use base64::Engine as _;
    let body_base64 = Some(base64::engine::general_purpose::STANDARD.encode(&body_bytes));

    let output = ProxyOutput {
        status,
        headers: resp_headers,
        body,
        body_base64,
    };

    let json = serde_json::to_string(&output)
        .map_err(|e| ExtismError::msg(format!("host_proxy_request: serialize: {e}")))?;
    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Host function: `host_http_request(input: {url, method, headers, body?, timeout_ms?}) -> {status, headers, body}`
///
/// Performs an HTTP request and returns the response. Only available when
/// the `http` feature is enabled (native builds). On WASM the browser
/// host functions provide the equivalent via `fetch()`.
#[cfg(feature = "http")]
fn host_http_request(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine as _;
    use ureq::http::Request;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct HttpInput {
        url: String,
        method: String,
        headers: std::collections::HashMap<String, String>,
        body: Option<String>,
        /// Base64-encoded binary body. Takes priority over `body` when present.
        body_base64: Option<String>,
        /// Optional request timeout in milliseconds.
        timeout_ms: Option<u64>,
    }

    #[derive(serde::Serialize)]
    struct HttpOutput {
        status: u16,
        headers: std::collections::HashMap<String, String>,
        body: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        body_base64: Option<String>,
    }

    /// Build an `HttpResponse`-shaped error envelope so guests can recover via
    /// the SDK's `http::request` parser instead of trapping.
    fn err_envelope(msg: &str) -> String {
        serde_json::json!({
            "status": 0,
            "headers": {},
            "body": msg,
            "error": msg,
        })
        .to_string()
    }

    let parsed: HttpInput = match serde_json::from_str(&input) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("host_http_request: invalid input: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };

    if let Err(e) = HostContext::validate_http_headers(&parsed.headers) {
        let msg = format!("host_http_request: {e}");
        plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
        return Ok(());
    }

    {
        let ctx = user_data.get()?;
        let ctx = ctx
            .lock()
            .map_err(|e| ExtismError::msg(format!("host_http_request: lock: {e}")))?;
        if let Err(e) = ctx.check_perm(PermissionType::HttpRequests, &parsed.url) {
            let msg = format!("host_http_request: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    }

    const MIN_HTTP_TIMEOUT_MS: u64 = 1_000;
    const MAX_HTTP_TIMEOUT_MS: u64 = 300_000;

    let timeout = parsed
        .timeout_ms
        .map(|value| value.clamp(MIN_HTTP_TIMEOUT_MS, MAX_HTTP_TIMEOUT_MS))
        .map(std::time::Duration::from_millis);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(timeout)
        .http_status_as_error(false)
        .build()
        .into();

    let mut request_builder = Request::builder()
        .method(parsed.method.as_str())
        .uri(parsed.url.as_str());
    for (key, value) in &parsed.headers {
        request_builder = request_builder.header(key, value);
    }

    let response = if let Some(b64) = &parsed.body_base64 {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(bytes) => bytes,
            Err(e) => {
                let msg = format!("host_http_request: base64 decode: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        };
        match request_builder.body(bytes) {
            Ok(request) => match agent.run(request) {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("host_http_request: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            },
            Err(e) => {
                let msg = format!("host_http_request: invalid request: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    } else if let Some(body) = &parsed.body {
        match request_builder.body(body.clone()) {
            Ok(request) => match agent.run(request) {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("host_http_request: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            },
            Err(e) => {
                let msg = format!("host_http_request: invalid request: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    } else {
        match request_builder.body(()) {
            Ok(request) => match agent.run(request) {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("host_http_request: {e}");
                    plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                    return Ok(());
                }
            },
            Err(e) => {
                let msg = format!("host_http_request: invalid request: {e}");
                plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
                return Ok(());
            }
        }
    };

    let status = response.status().as_u16();
    if status >= 400 {
        log::warn!(
            "host_http_request: {} {} → {} (plugin={})",
            parsed.method,
            parsed.url,
            status,
            {
                let ctx = user_data.get().ok();
                ctx.and_then(|c| c.lock().ok().map(|g| g.plugin_id.clone()))
                    .unwrap_or_default()
            },
        );
    }
    let mut resp_headers = std::collections::HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            resp_headers.insert(name.to_string(), value.to_string());
        }
    }
    let mut response = response;
    // Raise the default 10 MB body limit so plugins can download large WASM
    // binaries (e.g. pandoc.wasm ~58 MB).
    let body_bytes = match response
        .body_mut()
        .with_config()
        .limit(128 * 1024 * 1024)
        .read_to_vec()
    {
        Ok(bytes) => bytes,
        Err(e) => {
            let msg = format!("host_http_request: read body: {e}");
            plugin.memory_set_val(&mut outputs[0], err_envelope(&msg).as_str())?;
            return Ok(());
        }
    };
    let body = String::from_utf8_lossy(&body_bytes).to_string();
    let body_base64 = Some(base64::engine::general_purpose::STANDARD.encode(&body_bytes));

    let output = HttpOutput {
        status,
        headers: resp_headers,
        body,
        body_base64,
    };

    let json = serde_json::to_string(&output)
        .map_err(|e| ExtismError::msg(format!("host_http_request: serialize: {e}")))?;

    plugin.memory_set_val(&mut outputs[0], json.as_str())?;
    Ok(())
}

/// Stub for `host_http_request` when the `http` feature is not enabled.
#[cfg(not(feature = "http"))]
fn host_http_request(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let error = serde_json::json!({
        "status": 0,
        "headers": {},
        "body": "host_http_request: http feature not enabled"
    });
    plugin.memory_set_val(&mut outputs[0], error.to_string().as_str())?;
    Ok(())
}

#[cfg(not(feature = "http"))]
fn host_proxy_request(
    plugin: &mut CurrentPlugin,
    _inputs: &[Val],
    outputs: &mut [Val],
    _user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let error = serde_json::json!({
        "status": 0,
        "headers": {},
        "body": "host_proxy_request: http feature not enabled"
    });
    plugin.memory_set_val(&mut outputs[0], error.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_put_object(input: {ns_id, key, body_base64, mime_type, audience?}) -> {ok: true} or {error}`
fn host_namespace_put_object(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine as _;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        key: String,
        body_base64: String,
        mime_type: String,
        #[serde(default)]
        audience: Option<String>,
    }

    let parsed: Input = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_namespace_put_object: invalid input: {e}")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&parsed.body_base64)
        .map_err(|e| ExtismError::msg(format!("host_namespace_put_object: base64 decode: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_put_object: lock: {e}")))?;
    let result = ctx.namespace_provider.put_object(
        &parsed.ns_id,
        &parsed.key,
        &bytes,
        &parsed.mime_type,
        parsed.audience.as_deref(),
    );

    let json = match result {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_get_object(input: {ns_id, key}) -> {data: "<base64>"} or {error}`
fn host_namespace_get_object(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine as _;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        key: String,
    }

    let parsed: Input = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_namespace_get_object: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_get_object: lock: {e}")))?;
    let result = ctx
        .namespace_provider
        .get_object(&parsed.ns_id, &parsed.key);

    let json = match result {
        Ok(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            serde_json::json!({ "data": encoded })
        }
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_get_objects_batch(input: {ns_id, keys}) -> {objects: {key: {data, mime_type}}, errors: {key: msg}}`
fn host_namespace_get_objects_batch(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    use base64::Engine as _;

    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        keys: Vec<String>,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!(
            "host_namespace_get_objects_batch: invalid input: {e}"
        ))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_get_objects_batch: lock: {e}")))?;

    let result = ctx
        .namespace_provider
        .get_objects_batch(&parsed.ns_id, &parsed.keys);

    let json = match result {
        Ok(batch) => {
            let objects: serde_json::Map<String, serde_json::Value> = batch
                .objects
                .into_iter()
                .map(|(key, entry)| {
                    let is_text = entry.mime_type.starts_with("text/");
                    let val = if is_text {
                        serde_json::json!({
                            "data": String::from_utf8_lossy(&entry.bytes),
                            "mime_type": entry.mime_type,
                            "encoding": "text",
                        })
                    } else {
                        serde_json::json!({
                            "data": base64::engine::general_purpose::STANDARD.encode(&entry.bytes),
                            "mime_type": entry.mime_type,
                            "encoding": "base64",
                        })
                    };
                    (key, val)
                })
                .collect();

            let mut resp = serde_json::json!({ "objects": objects });
            if !batch.errors.is_empty() {
                resp["errors"] = serde_json::json!(batch.errors);
            }
            resp
        }
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_delete_object(input: {ns_id, key}) -> {ok: true} or {error}`
fn host_namespace_delete_object(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        key: String,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!("host_namespace_delete_object: invalid input: {e}"))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_delete_object: lock: {e}")))?;
    let result = ctx
        .namespace_provider
        .delete_object(&parsed.ns_id, &parsed.key);

    let json = match result {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_list_objects(input: {ns_id, prefix?, limit?, offset?}) -> [{key, audience?, mime_type?, size_bytes?, updated_at?, content_hash?}]`
fn host_namespace_list_objects(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        #[serde(default)]
        prefix: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        offset: Option<u32>,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!("host_namespace_list_objects: invalid input: {e}"))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_list_objects: lock: {e}")))?;
    let result = ctx.namespace_provider.list_objects(
        &parsed.ns_id,
        parsed.prefix.as_deref(),
        parsed.limit,
        parsed.offset,
    );

    let json = match result {
        Ok(objects) => serde_json::to_value(&objects).unwrap_or(serde_json::json!([])),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_list(input: {}) -> [NamespaceEntry] or {error}`
fn host_namespace_list(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let _input: String = plugin.memory_get_val(&inputs[0])?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_list: lock: {e}")))?;
    let result = ctx.namespace_provider.list_namespaces();

    let json = match result {
        Ok(entries) => serde_json::to_value(&entries).unwrap_or(serde_json::json!([])),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_create(input: {metadata?}) -> NamespaceEntry or {error}`
fn host_namespace_create(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    }

    let parsed: Input = serde_json::from_str(&input)
        .map_err(|e| ExtismError::msg(format!("host_namespace_create: invalid input: {e}")))?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_create: lock: {e}")))?;
    let result = ctx
        .namespace_provider
        .create_namespace(parsed.metadata.as_ref());

    let json = match result {
        Ok(entry) => serde_json::to_value(&entry).unwrap_or_else(|_| serde_json::json!({})),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_sync_audience(input: {ns_id, audience, gates}) -> {ok: true} or {error}`
fn host_namespace_sync_audience(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        audience: String,
        #[serde(default)]
        gates: serde_json::Value,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!("host_namespace_sync_audience: invalid input: {e}"))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_sync_audience: lock: {e}")))?;
    let result =
        ctx.namespace_provider
            .sync_audience(&parsed.ns_id, &parsed.audience, &parsed.gates);

    let json = match result {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_list_audiences(input: {ns_id}) -> {audiences: [name, ...]} or {error}`
fn host_namespace_list_audiences(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!("host_namespace_list_audiences: invalid input: {e}"))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_list_audiences: lock: {e}")))?;
    let result = ctx.namespace_provider.list_audiences(&parsed.ns_id);

    let json = match result {
        Ok(names) => serde_json::json!({ "audiences": names }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}

/// Host function: `host_namespace_delete_audience(input: {ns_id, audience}) -> {ok: true} or {error}`
fn host_namespace_delete_audience(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostContext>,
) -> Result<(), ExtismError> {
    let input: String = plugin.memory_get_val(&inputs[0])?;

    #[derive(serde::Deserialize)]
    struct Input {
        ns_id: String,
        audience: String,
    }

    let parsed: Input = serde_json::from_str(&input).map_err(|e| {
        ExtismError::msg(format!(
            "host_namespace_delete_audience: invalid input: {e}"
        ))
    })?;

    let ctx = user_data.get()?;
    let ctx = ctx
        .lock()
        .map_err(|e| ExtismError::msg(format!("host_namespace_delete_audience: lock: {e}")))?;
    let result = ctx
        .namespace_provider
        .delete_audience(&parsed.ns_id, &parsed.audience);

    let json = match result {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(e) => serde_json::json!({ "error": e }),
    };
    plugin.memory_set_val(&mut outputs[0], json.to_string().as_str())?;
    Ok(())
}
