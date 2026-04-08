//!
//! Tauri IPC command handlers
//!
//! These commands are callable from the frontend via Tauri's invoke system.
//!
//! All workspace operations go through the unified `execute()` command,
//! which routes to the appropriate handler in diaryx_core.
//!
//! Platform-specific commands (import) are handled separately as they
//! require Tauri plugins or system APIs.

use crate::logging;
#[cfg(target_os = "macos")]
use crate::macos_security_scoped::{
    ActiveSecurityScopedAccess, activate_security_scoped_bookmark, create_security_scoped_bookmark,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use diaryx_core::{
    Command,
    config::Config,
    diaryx::Diaryx,
    error::{DiaryxError, SerializableError},
    frontmatter,
    fs::{FileSystem, InMemoryFileSystem, RealFileSystem, SyncToAsyncFs},
    plugin::permissions::{PermissionRule, PermissionType, PluginConfig, PluginPermissions},
    workspace::Workspace,
};
#[cfg(feature = "extism-plugins")]
use diaryx_extism::protocol::{
    CommandResponse as ExtismCommandResponse, GuestRequestedPermissions,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;
#[cfg(all(
    feature = "desktop-updater",
    not(any(target_os = "android", target_os = "ios"))
))]
use tauri_plugin_updater::UpdaterExt;

// ============================================================================
// Types
// ============================================================================

/// App paths for different platforms
#[derive(Debug, Serialize)]
pub struct AppPaths {
    /// Directory for app data (config, etc.)
    pub data_dir: PathBuf,
    /// Directory for user documents/workspaces
    pub document_dir: PathBuf,
    /// Default workspace path
    pub default_workspace: PathBuf,
    /// Config file path
    pub config_path: PathBuf,
    /// Directory containing rolling application logs.
    pub log_dir: PathBuf,
    /// Active application log file.
    pub log_file: PathBuf,
    /// Whether this is a mobile platform (iOS/Android)
    pub is_mobile: bool,
    /// Whether this build targets Apple's App Store distribution path.
    pub is_apple_build: bool,
    /// iCloud workspace path (if iCloud is enabled and available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icloud_workspace: Option<PathBuf>,
    /// Whether iCloud storage is currently active for this session
    pub icloud_active: bool,
}

/// Probe result for the app's iCloud workspace container.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ICloudWorkspaceInfo {
    /// Whether iCloud Drive itself is available on this device/build.
    pub is_available: bool,
    /// Whether the Diaryx iCloud workspace already contains a root index.
    pub has_workspace: bool,
    /// Resolved iCloud workspace directory, when available.
    pub workspace_path: Option<PathBuf>,
    /// Display name for the discovered workspace, when present.
    pub workspace_name: Option<String>,
    /// Whether the current session is already using the iCloud workspace.
    pub active: bool,
}

/// Metadata for one workspace stored in the app's iCloud container.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ICloudWorkspaceRecord {
    pub workspace_id: String,
    pub workspace_name: String,
    pub workspace_path: PathBuf,
    pub active: bool,
}

/// Metadata for an available application update.
#[derive(Debug, Serialize)]
pub struct AppUpdateInfo {
    /// The semver version advertised by the updater endpoint.
    pub version: String,
    /// Optional release notes/body text from the updater manifest.
    pub body: Option<String>,
}

/// Base filesystem type for Tauri (real filesystem wrapped for async).
type TauriBaseFs = SyncToAsyncFs<RealFileSystem>;

/// Base filesystem type for guest mode (in-memory filesystem wrapped for async).
type GuestBaseFs = SyncToAsyncFs<InMemoryFileSystem>;

/// Global application state for the Tauri backend.
///
/// Stores the active workspace path and a cached Diaryx instance that is
/// reused across `execute()` calls for performance.
pub struct AppState {
    /// Path to the active workspace
    pub workspace_path: Mutex<Option<PathBuf>>,
    /// Cached Diaryx instance.
    pub diaryx: Mutex<Option<Arc<Diaryx<TauriBaseFs>>>>,
    /// When the plugin WASM files were last loaded into memory.
    /// Used to detect external updates (e.g. CLI `diaryx plugin update`).
    pub plugins_loaded_at: Mutex<Option<SystemTime>>,
    /// Active macOS security-scoped workspace access, when needed.
    #[cfg(target_os = "macos")]
    pub workspace_access: Mutex<Option<ActiveSecurityScopedAccess>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            workspace_path: Mutex::new(None),
            diaryx: Mutex::new(None),
            plugins_loaded_at: Mutex::new(None),
            #[cfg(target_os = "macos")]
            workspace_access: Mutex::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for guest mode - holds an in-memory filesystem when active.
///
/// When a Tauri user joins a share session as a guest, this state holds
/// the in-memory filesystem that all operations are routed through.
/// This prevents guest session files from affecting the user's local workspace.
pub struct GuestModeState {
    /// Whether guest mode is currently active
    pub active: Mutex<bool>,
    /// The in-memory filesystem used during guest mode
    pub filesystem: Mutex<Option<InMemoryFileSystem>>,
    /// The join code of the current guest session
    pub join_code: Mutex<Option<String>>,
    /// Cached Diaryx instance for guest mode.
    pub diaryx: Mutex<Option<Arc<Diaryx<GuestBaseFs>>>>,
}

impl GuestModeState {
    pub fn new() -> Self {
        Self {
            active: Mutex::new(false),
            filesystem: Mutex::new(None),
            join_code: Mutex::new(None),
            diaryx: Mutex::new(None),
        }
    }
}

impl Default for GuestModeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "extism-plugins")]
pub struct RuntimeContextState {
    pub context: Mutex<JsonValue>,
}

#[cfg(feature = "extism-plugins")]
impl RuntimeContextState {
    pub fn new() -> Self {
        Self {
            context: Mutex::new(serde_json::json!({})),
        }
    }
}

#[cfg(feature = "extism-plugins")]
impl Default for RuntimeContextState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInspection {
    pub plugin_id: String,
    pub plugin_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_permissions: Option<JsonValue>,
}

#[cfg(feature = "extism-plugins")]
struct TauriRequestFileProvider {
    pending: Mutex<HashMap<String, Vec<HashMap<String, Vec<u8>>>>>,
}

#[cfg(feature = "extism-plugins")]
impl TauriRequestFileProvider {
    fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn push(
        self: &Arc<Self>,
        plugin_id: &str,
        files: HashMap<String, Vec<u8>>,
    ) -> Result<TauriRequestFileScope, SerializableError> {
        let mut guard = self.pending.lock().map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Failed to lock plugin request files: {e}"),
            path: None,
        })?;
        guard.entry(plugin_id.to_string()).or_default().push(files);
        Ok(TauriRequestFileScope {
            provider: Arc::clone(self),
            plugin_id: plugin_id.to_string(),
        })
    }

    fn pop(&self, plugin_id: &str) {
        if let Ok(mut guard) = self.pending.lock()
            && let Some(stack) = guard.get_mut(plugin_id)
        {
            stack.pop();
            if stack.is_empty() {
                guard.remove(plugin_id);
            }
        }
    }
}

#[cfg(feature = "extism-plugins")]
impl diaryx_extism::FileProvider for TauriRequestFileProvider {
    fn get_file(&self, plugin_id: &str, key: &str) -> Option<Vec<u8>> {
        let guard = self.pending.lock().ok()?;
        guard
            .get(plugin_id)
            .and_then(|stack| stack.last())
            .and_then(|files| files.get(key))
            .cloned()
    }
}

#[cfg(feature = "extism-plugins")]
struct TauriRequestFileScope {
    provider: Arc<TauriRequestFileProvider>,
    plugin_id: String,
}

#[cfg(feature = "extism-plugins")]
impl Drop for TauriRequestFileScope {
    fn drop(&mut self) {
        self.provider.pop(&self.plugin_id);
    }
}

/// Helper function to safely acquire a mutex lock without panicking.
///
/// Returns a SerializableError if the mutex is poisoned, instead of panicking.
fn acquire_lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, SerializableError> {
    mutex.lock().map_err(|e| SerializableError {
        kind: "LockError".to_string(),
        message: format!("Failed to acquire lock: mutex is poisoned - {}", e),
        path: None,
    })
}

async fn save_config_file(config: &Config, config_path: &Path) -> Result<(), SerializableError> {
    config
        .save_to(&SyncToAsyncFs::new(RealFileSystem), config_path)
        .await
        .map_err(|e| e.to_serializable())
}

fn log_serializable_error(context: &str, err: &SerializableError) {
    if let Some(path) = err.path.as_ref() {
        log::error!(
            "[{}] {} (kind={}, path={})",
            context,
            err.message,
            err.kind,
            path.display()
        );
    } else {
        log::error!("[{}] {} (kind={})", context, err.message, err.kind);
    }
}

#[cfg(feature = "extism-plugins")]
fn log_plugin_install_error(err: SerializableError) -> SerializableError {
    log_serializable_error("install_user_plugin", &err);
    err
}

#[cfg(target_os = "macos")]
fn workspace_access_error(path: &Path, message: impl Into<String>) -> SerializableError {
    SerializableError {
        kind: "WorkspaceAccessError".to_string(),
        message: message.into(),
        path: Some(path.to_path_buf()),
    }
}

#[cfg(target_os = "macos")]
fn store_workspace_bookmark_in_config(
    config: &mut Config,
    workspace_path: &Path,
) -> Result<bool, SerializableError> {
    let bookmark = create_security_scoped_bookmark(workspace_path)
        .map_err(|e| workspace_access_error(workspace_path, e))?;
    if config.workspace_bookmark(workspace_path) == Some(bookmark.as_str()) {
        return Ok(false);
    }

    config.set_workspace_bookmark(workspace_path.to_path_buf(), bookmark);
    Ok(true)
}

#[cfg(target_os = "macos")]
fn activate_workspace_access_from_config(
    config: &mut Config,
    workspace_path: &Path,
) -> Result<Option<(ActiveSecurityScopedAccess, bool)>, SerializableError> {
    let Some(stored_bookmark) = config.workspace_bookmark(workspace_path).map(str::to_owned) else {
        return Ok(None);
    };

    let access = activate_security_scoped_bookmark(&stored_bookmark)
        .map_err(|e| workspace_access_error(workspace_path, e))?;
    let resolved_path = access.resolved_path().to_path_buf();
    let bookmark_to_store = access
        .refreshed_bookmark()
        .unwrap_or(stored_bookmark.as_str())
        .to_string();

    let mut changed = false;
    if config.workspace_bookmark(workspace_path) != Some(bookmark_to_store.as_str()) {
        config.set_workspace_bookmark(workspace_path.to_path_buf(), bookmark_to_store.clone());
        changed = true;
    }
    if config.workspace_bookmark(&resolved_path) != Some(bookmark_to_store.as_str()) {
        config.set_workspace_bookmark(resolved_path.clone(), bookmark_to_store);
        changed = true;
    }

    Ok(Some((access, changed)))
}

#[cfg(target_os = "macos")]
async fn load_workspace_config(config_path: &Path, default_workspace: &Path) -> Config {
    if config_path.exists() {
        Config::load_from(&SyncToAsyncFs::new(RealFileSystem), config_path)
            .await
            .unwrap_or_else(|_| Config::new(default_workspace.to_path_buf()))
    } else {
        Config::new(default_workspace.to_path_buf())
    }
}

#[cfg(target_os = "macos")]
fn try_backfill_workspace_bookmark(
    config: &mut Config,
    workspace_path: &Path,
    log_context: &str,
) -> bool {
    match store_workspace_bookmark_in_config(config, workspace_path) {
        Ok(changed) => {
            if changed {
                log::info!(
                    "[{}] Stored new security-scoped workspace bookmark for {:?}",
                    log_context,
                    workspace_path
                );
            }
            changed
        }
        Err(error) => {
            log::warn!(
                "[{}] Failed to create security-scoped workspace bookmark for {:?}: {}",
                log_context,
                workspace_path,
                error.message
            );
            false
        }
    }
}

/// Log command execution errors at the appropriate level.
///
/// `FileRead` with `NotFound` and `WorkspaceNotFound` are expected during
/// workspace initialization (e.g. theme settings not yet written, workspace
/// directory not fully set up) and are logged at `warn` to reduce noise.
fn log_execute_error(e: &DiaryxError) {
    match e {
        DiaryxError::FileRead { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            log::warn!("[execute] Command returned expected error: {:?}", e);
        }
        DiaryxError::WorkspaceNotFound(_) => {
            log::warn!("[execute] Command returned expected error: {:?}", e);
        }
        _ => {
            log::error!("[execute] Command execution failed: {:?}", e);
        }
    }
}

#[cfg(feature = "extism-plugins")]
fn workspace_has_root_index(root: &Path) -> bool {
    let workspace = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    futures_lite::future::block_on(workspace.find_root_index_in_dir(root))
        .ok()
        .flatten()
        .is_some()
}

#[cfg(feature = "extism-plugins")]
struct RootlessBootstrapPermissionChecker {
    workspace_root: PathBuf,
}

#[cfg(feature = "extism-plugins")]
impl diaryx_extism::PermissionChecker for RootlessBootstrapPermissionChecker {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission_type: PermissionType,
        target: &str,
    ) -> Result<(), String> {
        if !workspace_has_root_index(&self.workspace_root) {
            return Ok(());
        }

        let checker = diaryx_extism::FrontmatterPermissionChecker::from_workspace_root(Some(
            self.workspace_root.clone(),
        ));
        diaryx_extism::PermissionChecker::check_permission(
            &checker,
            plugin_id,
            permission_type,
            target,
        )
    }
}

#[cfg(feature = "extism-plugins")]
fn make_permission_checker(
    workspace_root: Option<PathBuf>,
) -> Arc<dyn diaryx_extism::PermissionChecker> {
    match workspace_root {
        Some(root) => {
            if workspace_has_root_index(&root) {
                Arc::new(
                    diaryx_extism::FrontmatterPermissionChecker::from_workspace_root(Some(root)),
                )
            } else {
                log::info!(
                    "Using allow-all plugin permissions while bootstrapping rootless workspace '{}'",
                    root.display()
                );
                Arc::new(RootlessBootstrapPermissionChecker {
                    workspace_root: root,
                })
            }
        }
        // No workspace root (e.g. during workspace download before it exists
        // on disk). The plugin is already installed and trusted, but there's no
        // frontmatter to read restrictions from.
        None => Arc::new(diaryx_extism::AllowAllPermissionChecker),
    }
}

#[cfg(feature = "extism-plugins")]
fn has_requested_permission_defaults(defaults: &PluginPermissions) -> bool {
    defaults.read_files.is_some()
        || defaults.edit_files.is_some()
        || defaults.create_files.is_some()
        || defaults.delete_files.is_some()
        || defaults.move_files.is_some()
        || defaults.http_requests.is_some()
        || defaults.execute_commands.is_some()
        || defaults.plugin_storage.is_some()
}

#[cfg(feature = "extism-plugins")]
fn merge_permission_rule(
    current: &mut Option<PermissionRule>,
    requested: &Option<PermissionRule>,
) -> bool {
    let Some(requested_rule) = requested else {
        return false;
    };

    let should_fill = match current {
        None => true,
        Some(rule) => rule.include.is_empty() && rule.exclude.is_empty(),
    };

    if !should_fill {
        return false;
    }

    *current = Some(requested_rule.clone());
    true
}

#[cfg(feature = "extism-plugins")]
fn merge_requested_permission_defaults(
    plugins_config: &mut HashMap<String, PluginConfig>,
    plugin_id: &str,
    defaults: &PluginPermissions,
) -> bool {
    if !has_requested_permission_defaults(defaults) {
        return false;
    }

    let plugin_config = plugins_config.entry(plugin_id.to_string()).or_default();
    let permissions = &mut plugin_config.permissions;
    let mut changed = false;

    changed |= merge_permission_rule(&mut permissions.read_files, &defaults.read_files);
    changed |= merge_permission_rule(&mut permissions.edit_files, &defaults.edit_files);
    changed |= merge_permission_rule(&mut permissions.create_files, &defaults.create_files);
    changed |= merge_permission_rule(&mut permissions.delete_files, &defaults.delete_files);
    changed |= merge_permission_rule(&mut permissions.move_files, &defaults.move_files);
    changed |= merge_permission_rule(&mut permissions.http_requests, &defaults.http_requests);
    changed |= merge_permission_rule(
        &mut permissions.execute_commands,
        &defaults.execute_commands,
    );
    changed |= merge_permission_rule(&mut permissions.plugin_storage, &defaults.plugin_storage);

    changed
}

#[cfg(feature = "extism-plugins")]
fn collect_requested_permissions(plugins_dir: &Path) -> HashMap<String, GuestRequestedPermissions> {
    let mut requested_permissions = HashMap::new();

    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!(
                "Failed to scan plugin manifests in '{}': {e}",
                plugins_dir.display()
            );
            return requested_permissions;
        }
    };

    for entry in entries.flatten() {
        let wasm_path = entry.path().join("plugin.wasm");
        if !wasm_path.exists() {
            continue;
        }

        match diaryx_extism::inspect_plugin_wasm_manifest(&wasm_path) {
            Ok(manifest) => {
                if let Some(requested) = manifest.requested_permissions
                    && has_requested_permission_defaults(&requested.defaults)
                {
                    requested_permissions.insert(manifest.id, requested);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to inspect requested permissions for '{}': {e}",
                    wasm_path.display()
                );
            }
        }
    }

    requested_permissions
}

#[cfg(feature = "extism-plugins")]
fn persist_requested_permission_defaults(
    workspace_root: &Path,
    requested_permissions: &HashMap<String, GuestRequestedPermissions>,
) -> Result<(), SerializableError> {
    if requested_permissions.is_empty() {
        return Ok(());
    }

    let workspace = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let root_index_path =
        futures_lite::future::block_on(workspace.find_root_index_in_dir(workspace_root))
            .map_err(|e: diaryx_core::error::DiaryxError| e.to_serializable())?;

    let Some(root_index_path) = root_index_path else {
        return Ok(());
    };

    let content = std::fs::read_to_string(&root_index_path).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!(
            "Failed to read root index '{}': {e}",
            root_index_path.display()
        ),
        path: Some(root_index_path.clone()),
    })?;

    let parsed = frontmatter::parse_or_empty(&content).map_err(|e| SerializableError {
        kind: "ValidationError".to_string(),
        message: format!(
            "Failed to parse root frontmatter for '{}': {e}",
            root_index_path.display()
        ),
        path: Some(root_index_path.clone()),
    })?;

    let mut plugins_config = match parsed.frontmatter.get("plugins") {
        Some(value) => serde_json::from_value::<HashMap<String, PluginConfig>>(
            serde_json::Value::from(value.clone()),
        )
        .map_err(|e| SerializableError {
            kind: "ValidationError".to_string(),
            message: format!(
                "Invalid plugin permissions in '{}': {e}",
                root_index_path.display()
            ),
            path: Some(root_index_path.clone()),
        })?,
        None => HashMap::new(),
    };

    let mut changed = false;
    for (plugin_id, requested) in requested_permissions {
        changed |= merge_requested_permission_defaults(
            &mut plugins_config,
            plugin_id,
            &requested.defaults,
        );
    }

    if !changed {
        return Ok(());
    }

    let plugins_value = {
        let json_val = serde_json::to_value(&plugins_config).map_err(|e| SerializableError {
            kind: "SerializationError".to_string(),
            message: format!("Failed to serialize plugin permissions: {e}"),
            path: Some(root_index_path.clone()),
        })?;
        diaryx_core::YamlValue::from(json_val)
    };

    futures_lite::future::block_on(workspace.set_frontmatter_property(
        &root_index_path,
        "plugins",
        plugins_value,
    ))
    .map_err(|e: diaryx_core::error::DiaryxError| e.to_serializable())
}

#[cfg(feature = "extism-plugins")]
fn persist_requested_permission_default(
    workspace_root: &Path,
    plugin_id: &str,
    requested: &GuestRequestedPermissions,
) -> Result<(), SerializableError> {
    let mut requested_permissions = HashMap::new();
    requested_permissions.insert(plugin_id.to_string(), requested.clone());
    persist_requested_permission_defaults(workspace_root, &requested_permissions)
}

// ============================================================================
// Extism Third-Party Plugin Loading
// ============================================================================

/// Resolve the workspace-local plugins directory: `{workspace_root}/.diaryx/plugins/`.
#[cfg(feature = "extism-plugins")]
fn workspace_plugins_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let app_state = app.state::<AppState>();
    let ws_path = app_state.workspace_path.lock().ok()?.clone()?;
    Some(ws_path.join(".diaryx").join("plugins"))
}

#[cfg(feature = "extism-plugins")]
fn make_plugin_storage(workspace_root: Option<PathBuf>) -> Arc<dyn diaryx_extism::PluginStorage> {
    match workspace_root {
        Some(root) => Arc::new(diaryx_extism::FilePluginStorage::new(
            root.join(".diaryx").join("plugin-state"),
        )),
        None => Arc::new(diaryx_extism::NoopStorage),
    }
}

#[cfg(feature = "extism-plugins")]
struct TauriPluginSecretStore {
    #[cfg(target_os = "android")]
    data_dir: Option<PathBuf>,
}

#[cfg(feature = "extism-plugins")]
impl diaryx_extism::PluginSecretStore for TauriPluginSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        #[cfg(not(target_os = "android"))]
        {
            match crate::credentials::get_credential(key.to_string()) {
                Ok(value) => value,
                Err(error) => {
                    log::warn!("Failed to load plugin secret from credential store: {error}");
                    None
                }
            }
        }

        #[cfg(target_os = "android")]
        {
            let Some(data_dir) = self.data_dir.as_ref() else {
                return None;
            };
            match crate::credentials::get_plugin_secret(data_dir, key) {
                Ok(value) => value,
                Err(error) => {
                    log::warn!("Failed to load plugin secret from credential store: {error}");
                    None
                }
            }
        }
    }

    fn set(&self, key: &str, value: &str) {
        #[cfg(not(target_os = "android"))]
        if let Err(error) = crate::credentials::store_credential(key.to_string(), value.to_string())
        {
            log::warn!("Failed to store plugin secret in credential store: {error}");
        }

        #[cfg(target_os = "android")]
        {
            let Some(data_dir) = self.data_dir.as_ref() else {
                return;
            };
            if let Err(error) = crate::credentials::store_plugin_secret(data_dir, key, value) {
                log::warn!("Failed to store plugin secret in credential store: {error}");
            }
        }
    }

    fn delete(&self, key: &str) {
        #[cfg(not(target_os = "android"))]
        if let Err(error) = crate::credentials::remove_credential(key.to_string()) {
            log::warn!("Failed to delete plugin secret from credential store: {error}");
        }

        #[cfg(target_os = "android")]
        {
            let Some(data_dir) = self.data_dir.as_ref() else {
                return;
            };
            if let Err(error) = crate::credentials::remove_plugin_secret(data_dir, key) {
                log::warn!("Failed to delete plugin secret from credential store: {error}");
            }
        }
    }
}

#[cfg(feature = "extism-plugins")]
fn make_plugin_secret_store<R: Runtime>(
    app: &AppHandle<R>,
) -> Arc<dyn diaryx_extism::PluginSecretStore> {
    #[cfg(not(target_os = "android"))]
    let _ = app;

    Arc::new(TauriPluginSecretStore {
        #[cfg(target_os = "android")]
        data_dir: app.path().app_data_dir().ok(),
    })
}

/// Load and register any third-party Extism WASM plugins from the workspace-local
/// plugin directory (`{workspace_root}/.diaryx/plugins/`).
///
/// Each subdirectory containing a `plugin.wasm` file is loaded as a plugin
/// and registered as both a WorkspacePlugin and FilePlugin. Errors during
/// loading are logged and skipped (not fatal).
///
/// Returns the loaded adapters so they can also be stored in [`PluginAdapters`]
/// for render IPC calls.
#[cfg(feature = "extism-plugins")]
fn register_extism_plugins<R: Runtime, FS: diaryx_core::fs::AsyncFileSystem + 'static>(
    app: &AppHandle<R>,
    diaryx: &mut Diaryx<FS>,
) -> Vec<Arc<diaryx_extism::ExtismPluginAdapter>> {
    let workspace_root = match diaryx.workspace_root() {
        Some(root) => root,
        None => return Vec::new(),
    };
    let plugins_dir = workspace_root.join(".diaryx").join("plugins");
    if !plugins_dir.exists() {
        return Vec::new();
    }
    let requested_permissions = collect_requested_permissions(&plugins_dir);

    // Use a basic real filesystem for host function file access.
    let fs: Arc<dyn diaryx_core::fs::AsyncFileSystem> =
        Arc::new(SyncToAsyncFs::new(RealFileSystem));
    let workspace_root_opt = diaryx.workspace_root();
    let event_emitter: Arc<dyn diaryx_extism::EventEmitter> =
        Arc::new(TauriEventEmitter { app: app.clone() });
    let file_provider: Arc<dyn diaryx_extism::FileProvider> =
        app.state::<PluginAdapters>().file_provider.clone();
    let ws_bridge = Arc::new(diaryx_extism::TokioWebSocketBridge::new());
    let host_ctx = Arc::new(diaryx_extism::HostContext {
        fs,
        storage: make_plugin_storage(workspace_root_opt.clone()),
        secret_store: make_plugin_secret_store(app),
        event_emitter,
        plugin_id: String::new(),
        plugin_id_locked: false,
        permission_checker: Some(make_permission_checker(workspace_root_opt)),
        file_provider,
        ws_bridge: ws_bridge.clone(),
        plugin_command_bridge: Arc::new(TauriPluginCommandBridge { app: app.clone() }),
        runtime_context_provider: Arc::new(TauriRuntimeContextProvider { app: app.clone() }),
        namespace_provider: Arc::new(TauriNamespaceProvider { app: app.clone() }),
        plugin_command_depth: 0,
        storage_quota_bytes: diaryx_extism::DEFAULT_STORAGE_QUOTA_BYTES,
    });
    let mut adapters = Vec::new();
    match diaryx_extism::load_plugins_from_dir(&plugins_dir, host_ctx) {
        Ok(plugins) => {
            if let Err(e) =
                persist_requested_permission_defaults(&workspace_root, &requested_permissions)
            {
                log::warn!(
                    "Failed to persist requested plugin permissions for '{}': {}",
                    workspace_root.display(),
                    e.message
                );
            }
            use diaryx_core::plugin::Plugin;
            for plugin in plugins {
                let arc = Arc::new(plugin);
                if arc
                    .manifest()
                    .capabilities
                    .iter()
                    .any(|cap| matches!(cap, diaryx_core::plugin::PluginCapability::SyncTransport))
                {
                    let sync_guest: Arc<dyn diaryx_extism::SyncGuestBridge> = arc.clone();
                    ws_bridge.set_guest_bridge(Arc::downgrade(&sync_guest));
                }
                diaryx
                    .plugin_registry_mut()
                    .register_workspace_plugin(arc.clone());
                diaryx
                    .plugin_registry_mut()
                    .register_file_plugin(arc.clone());
                adapters.push(arc);
            }
        }
        Err(e) => {
            log::warn!("Failed to load extism plugins: {e}");
        }
    }
    adapters
}

// ============================================================================
// Extism Host Bridges
// ============================================================================

/// Tauri event emitter for Extism plugins.
///
/// Uppercase `type` payloads are forwarded as generic filesystem events.
#[cfg(feature = "extism-plugins")]
struct TauriEventEmitter<R: Runtime> {
    app: AppHandle<R>,
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> diaryx_extism::EventEmitter for TauriEventEmitter<R> {
    fn emit(&self, event_json: &str) {
        if let Ok(event) = serde_json::from_str::<JsonValue>(event_json)
            && event
                .get("type")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value.chars().next().is_some_and(|ch| ch.is_uppercase()))
        {
            let _ = self.app.emit("extism-filesystem-event", event_json);
        }

        let _ = self.app.emit("extism-plugin-event", event_json);
    }
}

#[cfg(feature = "extism-plugins")]
struct TauriPluginCommandBridge<R: Runtime> {
    app: AppHandle<R>,
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> diaryx_extism::PluginCommandBridge for TauriPluginCommandBridge<R> {
    fn call(
        &self,
        caller_plugin_id: &str,
        plugin_id: &str,
        command: &str,
        params: JsonValue,
    ) -> Result<JsonValue, String> {
        if caller_plugin_id == plugin_id {
            return Err("Plugins cannot call their own commands via host_plugin_command".into());
        }

        let adapters = self.app.state::<PluginAdapters>();
        let guard = adapters
            .adapters
            .lock()
            .map_err(|e| format!("Failed to lock plugin adapters: {e}"))?;
        let adapter = guard
            .get(plugin_id)
            .ok_or_else(|| format!("Plugin '{plugin_id}' is not loaded"))?;

        let input = serde_json::json!({
            "command": command,
            "params": params,
        })
        .to_string();
        let output = adapter
            .call_guest("handle_command", &input)
            .map_err(|e| e.to_string())?;
        let response = serde_json::from_str::<JsonValue>(&output)
            .map_err(|e| format!("Invalid plugin response: {e}"))?;

        if response.get("success").and_then(|value| value.as_bool()) == Some(true) {
            Ok(response.get("data").cloned().unwrap_or(JsonValue::Null))
        } else {
            Err(response
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("Unknown plugin error")
                .to_string())
        }
    }
}

#[cfg(feature = "extism-plugins")]
struct TauriRuntimeContextProvider<R: Runtime> {
    app: AppHandle<R>,
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> diaryx_extism::RuntimeContextProvider for TauriRuntimeContextProvider<R> {
    fn get_context(&self, _plugin_id: &str) -> JsonValue {
        self.app
            .try_state::<RuntimeContextState>()
            .and_then(|state| state.context.lock().ok().map(|value| value.clone()))
            .unwrap_or_else(|| serde_json::json!({}))
    }
}

#[cfg(feature = "extism-plugins")]
struct TauriNamespaceProvider<R: Runtime> {
    app: AppHandle<R>,
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> TauriNamespaceProvider<R> {
    fn runtime_context(&self) -> JsonValue {
        self.app
            .try_state::<RuntimeContextState>()
            .and_then(|state| state.context.lock().ok().map(|value| value.clone()))
            .unwrap_or_else(|| serde_json::json!({}))
    }

    fn server_url(&self) -> Result<String, String> {
        self.runtime_context()
            .get("server_url")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_end_matches('/').to_string())
            .ok_or_else(|| "Namespace operations require runtime_context.server_url".to_string())
    }

    fn auth_token(&self) -> Option<String> {
        self.runtime_context()
            .get("auth_token")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn encode_component(value: &str) -> String {
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
    }

    fn encode_key(key: &str) -> String {
        key.split('/')
            .map(Self::encode_component)
            .collect::<Vec<_>>()
            .join("/")
    }

    fn request_bytes(&self, url: String) -> Result<Vec<u8>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .into();

        let mut request_builder = ureq::http::Request::builder()
            .method("GET")
            .uri(url.as_str());
        if let Some(token) = self.auth_token() {
            request_builder = request_builder.header("Authorization", format!("Bearer {token}"));
        }

        let request = request_builder
            .body(())
            .map_err(|e| format!("Failed to build namespace request: {e}"))?;
        let response = agent
            .run(request)
            .map_err(|e| format!("Namespace request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.into_body().read_to_string().unwrap_or_default();
            return Err(if text.is_empty() {
                format!("Namespace request failed with status {status}")
            } else {
                text
            });
        }
        response
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("Failed to read namespace response: {e}"))
    }

    fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        url: String,
        body: Option<Vec<u8>>,
        content_type: Option<&str>,
        audience: Option<&str>,
    ) -> Result<Option<T>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .into();

        let mut request_builder = ureq::http::Request::builder()
            .method(method)
            .uri(url.as_str());
        if let Some(token) = self.auth_token() {
            request_builder = request_builder.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(content_type) = content_type {
            request_builder = request_builder.header("Content-Type", content_type);
        }
        if let Some(audience) = audience {
            request_builder = request_builder.header("X-Audience", audience);
        }

        let response = if let Some(body) = body {
            let request = request_builder
                .body(body)
                .map_err(|e| format!("Failed to build namespace request: {e}"))?;
            agent
                .run(request)
                .map_err(|e| format!("Namespace request failed: {e}"))?
        } else {
            let request = request_builder
                .body(())
                .map_err(|e| format!("Failed to build namespace request: {e}"))?;
            agent
                .run(request)
                .map_err(|e| format!("Namespace request failed: {e}"))?
        };
        let status = response.status();
        if !status.is_success() {
            let text = response.into_body().read_to_string().unwrap_or_default();
            return Err(if text.is_empty() {
                format!("Namespace request failed with status {status}")
            } else {
                text
            });
        }
        if status == ureq::http::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let bytes = response
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("Failed to read namespace response: {e}"))?;
        if bytes.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice::<T>(&bytes)
            .map(Some)
            .map_err(|e| format!("Failed to parse namespace response JSON: {e}"))
    }
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> diaryx_extism::NamespaceProvider for TauriNamespaceProvider<R> {
    fn create_namespace(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Result<diaryx_extism::NamespaceEntry, String> {
        let base = self.server_url()?;
        let url = format!("{}/namespaces", base);
        let body = serde_json::to_vec(&serde_json::json!({ "metadata": metadata }))
            .map_err(|e| format!("Failed to serialize namespace request: {e}"))?;
        self.request_json::<diaryx_extism::NamespaceEntry>(
            "POST",
            url,
            Some(body),
            Some("application/json"),
            None,
        )?
        .ok_or_else(|| "Namespace create returned an empty response".to_string())
    }

    fn put_object(
        &self,
        ns_id: &str,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        audience: Option<&str>,
    ) -> Result<(), String> {
        let base = self.server_url()?;
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            base,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_json::<serde_json::Value>(
            "PUT",
            url,
            Some(bytes.to_vec()),
            Some(mime_type),
            audience,
        )?;
        Ok(())
    }

    fn get_object(&self, ns_id: &str, key: &str) -> Result<Vec<u8>, String> {
        let base = self.server_url()?;
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            base,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_bytes(url)
    }

    fn delete_object(&self, ns_id: &str, key: &str) -> Result<(), String> {
        let base = self.server_url()?;
        let url = format!(
            "{}/namespaces/{}/objects/{}",
            base,
            Self::encode_component(ns_id),
            Self::encode_key(key)
        );
        self.request_json::<serde_json::Value>("DELETE", url, None, None, None)?;
        Ok(())
    }

    fn list_objects(
        &self,
        ns_id: &str,
        prefix: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<diaryx_extism::NamespaceObjectMeta>, String> {
        let base = self.server_url()?;
        let mut url = format!(
            "{}/namespaces/{}/objects",
            base,
            Self::encode_component(ns_id)
        );
        let mut query = Vec::new();
        if let Some(prefix) = prefix {
            query.push(format!("prefix={}", Self::encode_component(prefix)));
        }
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if let Some(offset) = offset {
            query.push(format!("offset={offset}"));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        Ok(self
            .request_json::<Vec<diaryx_extism::NamespaceObjectMeta>>("GET", url, None, None, None)?
            .unwrap_or_default())
    }

    fn sync_audience(&self, ns_id: &str, audience: &str, access: &str) -> Result<(), String> {
        let base = self.server_url()?;
        let url = format!(
            "{}/namespaces/{}/audiences/{}",
            base,
            Self::encode_component(ns_id),
            Self::encode_component(audience)
        );
        let body = serde_json::to_vec(&serde_json::json!({ "access": access }))
            .map_err(|e| format!("Failed to serialize audience request: {e}"))?;
        self.request_json::<serde_json::Value>(
            "PUT",
            url,
            Some(body),
            Some("application/json"),
            None,
        )?;
        Ok(())
    }

    fn list_namespaces(&self) -> Result<Vec<diaryx_extism::NamespaceEntry>, String> {
        let base = self.server_url()?;
        let url = format!("{}/namespaces", base);
        Ok(self
            .request_json::<Vec<diaryx_extism::NamespaceEntry>>("GET", url, None, None, None)?
            .unwrap_or_default())
    }

    fn send_audience_email(
        &self,
        ns_id: &str,
        audience: &str,
        subject: &str,
        reply_to: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let base = self.server_url()?;
        let url = format!(
            "{}/namespaces/{}/audiences/{}/send-email",
            base,
            Self::encode_component(ns_id),
            Self::encode_component(audience)
        );
        let body = serde_json::to_vec(&serde_json::json!({
            "subject": subject,
            "reply_to": reply_to,
        }))
        .map_err(|e| format!("Failed to serialize send-email request: {e}"))?;
        Ok(self
            .request_json::<serde_json::Value>(
                "POST",
                url,
                Some(body),
                Some("application/json"),
                None,
            )?
            .unwrap_or_else(|| serde_json::json!({ "ok": true })))
    }
}

/// Holds loaded [`ExtismPluginAdapter`] instances by plugin ID for render IPC calls.
///
/// The frontend can call `call_plugin_render` to invoke a plugin's render export
/// (e.g., math rendering) without needing browser Extism support.
#[cfg(feature = "extism-plugins")]
pub struct PluginAdapters {
    pub adapters: Mutex<HashMap<String, Arc<diaryx_extism::ExtismPluginAdapter>>>,
    file_provider: Arc<TauriRequestFileProvider>,
}

#[cfg(feature = "extism-plugins")]
impl PluginAdapters {
    pub fn new() -> Self {
        Self {
            adapters: Mutex::new(HashMap::new()),
            file_provider: Arc::new(TauriRequestFileProvider::new()),
        }
    }
}

#[cfg(feature = "extism-plugins")]
impl Default for PluginAdapters {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// User Plugin Install/Uninstall
// ============================================================================

/// Inspect a user plugin from raw WASM bytes without installing it.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn inspect_user_plugin<R: Runtime>(
    _app: AppHandle<R>,
    wasm_bytes: Vec<u8>,
) -> Result<PluginInspection, SerializableError> {
    let tmp_dir = std::env::temp_dir().join("diaryx-plugin-inspect");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to create temp directory: {e}"),
        path: None,
    })?;
    let tmp_wasm = tmp_dir.join("plugin.wasm");
    std::fs::write(&tmp_wasm, &wasm_bytes).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to write temp WASM: {e}"),
        path: None,
    })?;

    let result = diaryx_extism::inspect_plugin_wasm_manifest(&tmp_wasm)
        .map(|manifest| PluginInspection {
            plugin_id: manifest.id,
            plugin_name: manifest.name,
            requested_permissions: manifest
                .requested_permissions
                .and_then(|value| serde_json::to_value(value).ok()),
        })
        .map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Invalid WASM plugin: {e}"),
            path: None,
        });

    let _ = std::fs::remove_dir_all(&tmp_dir);
    result
}

/// Install a user plugin from raw WASM bytes.
///
/// Writes the WASM to `~/.diaryx/plugins/{plugin_id}/plugin.wasm`, loads it
/// to extract the manifest, then clears the cached Diaryx instance so the
/// plugin is picked up on the next `execute()` call.
///
/// Returns the plugin manifest as a JSON string.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn install_user_plugin<R: Runtime>(
    app: AppHandle<R>,
    wasm_bytes: Vec<u8>,
) -> Result<String, SerializableError> {
    use diaryx_core::plugin::Plugin;

    log::info!(
        "[install_user_plugin] Installing plugin ({} KB)",
        wasm_bytes.len() / 1024
    );

    // Write to a temp location first so we can call load_plugin_from_wasm (file-based).
    let tmp_dir = std::env::temp_dir().join("diaryx-plugin-install");
    let tmp_wasm = tmp_dir.join("plugin.wasm");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| {
        log_plugin_install_error(SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create temp plugin install directory: {e}"),
            path: Some(tmp_dir.clone()),
        })
    })?;
    log::info!(
        "[install_user_plugin] Writing temp plugin WASM to {}",
        tmp_wasm.display()
    );
    std::fs::write(&tmp_wasm, &wasm_bytes).map_err(|e| {
        log_plugin_install_error(SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to write temp plugin WASM: {e}"),
            path: Some(tmp_wasm.clone()),
        })
    })?;
    let requested_permissions = match diaryx_extism::inspect_plugin_wasm_manifest(&tmp_wasm) {
        Ok(manifest) => manifest.requested_permissions,
        Err(err) => {
            log::warn!(
                "[install_user_plugin] Failed to inspect requested permissions from {}: {}",
                tmp_wasm.display(),
                err
            );
            None
        }
    };

    // Load to extract the manifest.
    let fs: Arc<dyn diaryx_core::fs::AsyncFileSystem> =
        Arc::new(SyncToAsyncFs::new(RealFileSystem));
    let host_ctx = Arc::new(diaryx_extism::HostContext {
        fs,
        storage: Arc::new(diaryx_extism::NoopStorage),
        secret_store: Arc::new(diaryx_extism::NoopSecretStore),
        event_emitter: Arc::new(diaryx_extism::NoopEventEmitter),
        plugin_id: String::new(),
        plugin_id_locked: false,
        permission_checker: Some(Arc::new(diaryx_extism::DenyAllPermissionChecker)),
        file_provider: Arc::new(diaryx_extism::NoopFileProvider),
        ws_bridge: Arc::new(diaryx_extism::NoopWebSocketBridge),
        plugin_command_bridge: Arc::new(diaryx_extism::NoopPluginCommandBridge),
        runtime_context_provider: Arc::new(diaryx_extism::NoopRuntimeContextProvider),
        namespace_provider: Arc::new(diaryx_extism::NoopNamespaceProvider),
        plugin_command_depth: 0,
        storage_quota_bytes: diaryx_extism::DEFAULT_STORAGE_QUOTA_BYTES,
    });

    log::info!(
        "[install_user_plugin] Loading plugin manifest from {}",
        tmp_wasm.display()
    );
    let adapter = diaryx_extism::load_plugin_from_wasm(&tmp_wasm, host_ctx, None).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        log_plugin_install_error(SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Invalid WASM plugin while loading manifest: {e}"),
            path: Some(tmp_wasm.clone()),
        })
    })?;

    let manifest = adapter.manifest();
    let plugin_id = manifest.id.0.clone();
    log::info!(
        "[install_user_plugin] Parsed plugin manifest: {} ({})",
        manifest.name,
        plugin_id
    );
    let manifest_json = serde_json::to_string(&manifest).map_err(|e| {
        log_plugin_install_error(SerializableError {
            kind: "SerializationError".to_string(),
            message: format!("Failed to serialize plugin manifest: {e}"),
            path: Some(tmp_wasm.clone()),
        })
    })?;

    // Persist WASM to {workspace_root}/.diaryx/plugins/{plugin_id}/plugin.wasm
    let base_dir = workspace_plugins_dir(&app).ok_or_else(|| {
        log_plugin_install_error(SerializableError {
            kind: "NotFound".to_string(),
            message: "No workspace is open — cannot install plugin".to_string(),
            path: None,
        })
    })?;
    log::info!(
        "[install_user_plugin] Installing '{}' into {}",
        plugin_id,
        base_dir.display()
    );
    let plugins_dir = base_dir.join(&plugin_id);
    std::fs::create_dir_all(&plugins_dir).map_err(|e| {
        log_plugin_install_error(SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create plugin directory: {e}"),
            path: Some(plugins_dir.clone()),
        })
    })?;

    let wasm_path = plugins_dir.join("plugin.wasm");
    log::info!(
        "[install_user_plugin] Writing installed plugin WASM to {}",
        wasm_path.display()
    );
    std::fs::rename(&tmp_wasm, &wasm_path)
        .or_else(|rename_err| {
            // rename fails across filesystems; fall back to copy+delete
            log::warn!(
                "[install_user_plugin] Failed to move temp WASM into place ({}); falling back to copy",
                rename_err
            );
            std::fs::copy(&tmp_wasm, &wasm_path).map(|_| ())
        })
        .map_err(|e| {
            log_plugin_install_error(SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to write plugin WASM into workspace: {e}"),
                path: Some(wasm_path.clone()),
            })
        })?;
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // Remove stale manifest.json cache so that the next plugin load re-reads
    // the manifest from the updated WASM binary instead of the old cache.
    let cached_manifest = plugins_dir.join("manifest.json");
    if cached_manifest.exists() {
        log::info!(
            "[install_user_plugin] Removing stale manifest cache at {}",
            cached_manifest.display()
        );
        let _ = std::fs::remove_file(&cached_manifest);
    }

    if let Some(requested) = requested_permissions.as_ref()
        && has_requested_permission_defaults(&requested.defaults)
    {
        if let Some(workspace_root) = base_dir.parent().and_then(|path| path.parent()) {
            log::info!(
                "[install_user_plugin] Persisting requested permission defaults for '{}' into {}",
                plugin_id,
                workspace_root.display()
            );
        }
        if let Some(workspace_root) = base_dir.parent().and_then(|path| path.parent())
            && let Err(e) =
                persist_requested_permission_default(workspace_root, &plugin_id, requested)
        {
            log::warn!(
                "[install_user_plugin] Failed to persist requested plugin permissions for '{}' during install: {}",
                plugin_id,
                e.message
            );
        }
    }

    // Clear cached Diaryx so next execute() picks up the new plugin.
    let app_state = app.state::<AppState>();
    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        log::info!(
            "[install_user_plugin] Clearing cached Diaryx instance after installing {}",
            plugin_id
        );
        *diaryx_guard = None;
        let mut loaded_guard = acquire_lock(&app_state.plugins_loaded_at)?;
        *loaded_guard = None;
    }

    log::info!(
        "[install_user_plugin] Installed: {} ({})",
        manifest.name,
        plugin_id
    );
    Ok(manifest_json)
}

/// Uninstall a user plugin by ID.
///
/// Deletes `{workspace_root}/.diaryx/plugins/{plugin_id}/` and clears the cached Diaryx instance.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn uninstall_user_plugin<R: Runtime>(
    app: AppHandle<R>,
    plugin_id: String,
) -> Result<(), SerializableError> {
    log::info!("[uninstall_user_plugin] Uninstalling plugin: {}", plugin_id);

    let base_dir = workspace_plugins_dir(&app).ok_or_else(|| SerializableError {
        kind: "NotFound".to_string(),
        message: "No workspace is open — cannot uninstall plugin".to_string(),
        path: None,
    })?;
    let plugins_dir = base_dir.join(&plugin_id);

    if plugins_dir.exists() {
        std::fs::remove_dir_all(&plugins_dir).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to remove plugin directory: {e}"),
            path: Some(plugins_dir.clone()),
        })?;
    }

    // Clear cached Diaryx so the plugin is no longer registered.
    let app_state = app.state::<AppState>();
    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        *diaryx_guard = None;
    }

    log::info!("[uninstall_user_plugin] Uninstalled: {}", plugin_id);
    Ok(())
}

/// Execute a plugin command with temporary host-provided file bytes.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn execute_plugin_command_with_files<R: Runtime>(
    app: AppHandle<R>,
    plugin_id: String,
    command: String,
    params: JsonValue,
    request_files: HashMap<String, Vec<u8>>,
) -> Result<JsonValue, SerializableError> {
    let adapters = app.state::<PluginAdapters>();
    let adapter = {
        let guard = adapters.adapters.lock().map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Failed to lock plugin adapters: {e}"),
            path: None,
        })?;
        guard
            .get(&plugin_id)
            .cloned()
            .ok_or_else(|| SerializableError {
                kind: "PluginError".to_string(),
                message: format!("Plugin '{plugin_id}' is not loaded"),
                path: None,
            })?
    };

    let _request_scope = adapters.file_provider.push(&plugin_id, request_files)?;
    let input = serde_json::json!({
        "command": command,
        "params": params,
    })
    .to_string();
    let output = adapter
        .call_guest("handle_command", &input)
        .map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: e.to_string(),
            path: None,
        })?;
    let response =
        serde_json::from_str::<ExtismCommandResponse>(&output).map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Invalid plugin response: {e}"),
            path: None,
        })?;

    if response.success {
        Ok(response.data.unwrap_or(JsonValue::Null))
    } else {
        Err(SerializableError {
            kind: "PluginError".to_string(),
            message: response
                .error
                .unwrap_or_else(|| "Unknown plugin error".to_string()),
            path: None,
        })
    }
}

#[cfg(feature = "extism-plugins")]
fn extract_component_html_value(value: JsonValue) -> Option<String> {
    match value {
        JsonValue::String(html) => Some(html),
        JsonValue::Object(mut obj) => {
            if let Some(html) = obj
                .get("response")
                .and_then(|value| value.as_str())
                .or_else(|| obj.get("html").and_then(|value| value.as_str()))
                .or_else(|| obj.get("data").and_then(|value| value.as_str()))
            {
                return Some(html.to_string());
            }

            if obj.get("type").and_then(|value| value.as_str()) == Some("PluginResult") {
                return obj.remove("data").and_then(extract_component_html_value);
            }

            if obj.get("success").and_then(|value| value.as_bool()) == Some(true) {
                return obj.remove("data").and_then(extract_component_html_value);
            }

            None
        }
        _ => None,
    }
}

#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn get_plugin_component_html<R: Runtime>(
    app: AppHandle<R>,
    plugin_id: String,
    component_id: String,
) -> Result<String, SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    if *acquire_lock(&guest_state.active)? {
        return Err(SerializableError {
            kind: "GuestModeError".to_string(),
            message: "Native plugin components are unavailable in guest mode".to_string(),
            path: None,
        });
    }

    let _ = get_or_init_tauri_diaryx(&app).await?;

    let adapters = app.state::<PluginAdapters>();
    let adapter = {
        let guard = adapters.adapters.lock().map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Failed to lock plugin adapters: {e}"),
            path: None,
        })?;
        guard
            .get(&plugin_id)
            .cloned()
            .ok_or_else(|| SerializableError {
                kind: "PluginError".to_string(),
                message: format!("Plugin '{plugin_id}' is not loaded"),
                path: None,
            })?
    };

    let direct_input = serde_json::json!({
        "component_id": component_id,
    })
    .to_string();
    if let Ok(output) = adapter.call_guest("get_component_html", &direct_input) {
        return Ok(output);
    }

    let fallback_input = serde_json::json!({
        "command": "get_component_html",
        "params": {
            "component_id": component_id,
        },
    })
    .to_string();
    let output = adapter
        .call_guest("handle_command", &fallback_input)
        .map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: e.to_string(),
            path: None,
        })?;
    let response =
        serde_json::from_str::<ExtismCommandResponse>(&output).map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Invalid plugin response: {e}"),
            path: None,
        })?;

    if response.success {
        if let Some(html) = response.data.and_then(extract_component_html_value) {
            Ok(html)
        } else {
            Err(SerializableError {
                kind: "PluginError".to_string(),
                message: format!("Plugin '{plugin_id}' returned invalid component HTML"),
                path: None,
            })
        }
    } else {
        Err(SerializableError {
            kind: "PluginError".to_string(),
            message: response
                .error
                .unwrap_or_else(|| "Unknown plugin error".to_string()),
            path: None,
        })
    }
}

/// Stub: install_user_plugin when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn install_user_plugin<R: Runtime>(
    _app: AppHandle<R>,
    _wasm_bytes: Vec<u8>,
) -> Result<String, SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

/// Stub: inspect_user_plugin when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn inspect_user_plugin<R: Runtime>(
    _app: AppHandle<R>,
    _wasm_bytes: Vec<u8>,
) -> Result<PluginInspection, SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

/// Stub: uninstall_user_plugin when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn uninstall_user_plugin<R: Runtime>(
    _app: AppHandle<R>,
    _plugin_id: String,
) -> Result<(), SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

/// Stub: execute_plugin_command_with_files when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn execute_plugin_command_with_files<R: Runtime>(
    _app: AppHandle<R>,
    _plugin_id: String,
    _command: String,
    _params: JsonValue,
    _request_files: HashMap<String, Vec<u8>>,
) -> Result<JsonValue, SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

/// Stub: get_plugin_component_html when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn get_plugin_component_html<R: Runtime>(
    _app: AppHandle<R>,
    _plugin_id: String,
    _component_id: String,
) -> Result<String, SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

// ============================================================================
// Plugin Render IPC
// ============================================================================

/// Call a plugin's render export function via IPC.
///
/// Used by the frontend to render plugin content (e.g., math blocks) when
/// browser Extism plugins aren't available (iOS) or haven't loaded yet.
/// The plugin must have been loaded by `register_extism_plugins` and stored
/// in [`PluginAdapters`].
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn call_plugin_render<R: Runtime>(
    app: AppHandle<R>,
    plugin_id: String,
    export_name: String,
    input: String,
) -> Result<String, SerializableError> {
    let adapters = app.state::<PluginAdapters>();
    let guard = adapters.adapters.lock().map_err(|e| SerializableError {
        kind: "LockError".to_string(),
        message: format!("Failed to lock plugin adapters: {e}"),
        path: None,
    })?;
    let adapter = guard.get(&plugin_id).ok_or_else(|| SerializableError {
        kind: "NotFound".to_string(),
        message: format!("Plugin '{}' not loaded", plugin_id),
        path: None,
    })?;
    adapter
        .call_guest(&export_name, &input)
        .map_err(|e| SerializableError {
            kind: "PluginError".to_string(),
            message: e.to_string(),
            path: None,
        })
}

/// Stub: call_plugin_render when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn call_plugin_render<R: Runtime>(
    _app: AppHandle<R>,
    _plugin_id: String,
    _export_name: String,
    _input: String,
) -> Result<String, SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub fn set_runtime_context<R: Runtime>(
    app: AppHandle<R>,
    context_json: String,
) -> Result<(), SerializableError> {
    let context: JsonValue =
        serde_json::from_str(&context_json).map_err(|e| SerializableError {
            kind: "ParseError".to_string(),
            message: format!("Failed to parse runtime context JSON: {e}"),
            path: None,
        })?;
    let state = app.state::<RuntimeContextState>();
    let mut guard = acquire_lock(&state.context)?;
    *guard = context;
    Ok(())
}

#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub fn set_runtime_context<R: Runtime>(
    _app: AppHandle<R>,
    _context_json: String,
) -> Result<(), SerializableError> {
    Ok(())
}

#[cfg(feature = "extism-plugins")]
fn sync_loaded_plugin_adapters<R: Runtime>(
    app: &AppHandle<R>,
    adapters: Vec<Arc<diaryx_extism::ExtismPluginAdapter>>,
) {
    if let Some(plugin_adapters) = app.try_state::<PluginAdapters>()
        && let Ok(mut guard) = plugin_adapters.adapters.lock()
    {
        guard.clear();
        for adapter in adapters {
            use diaryx_core::plugin::Plugin;
            guard.insert(adapter.manifest().id.0.clone(), adapter);
        }
    }
}

/// Check whether any `plugin.wasm` file under the workspace plugins directory
/// has been modified since we last loaded plugins. Returns `true` if the cache
/// should be invalidated.
fn plugins_changed_on_disk(workspace_path: Option<&Path>, loaded_at: Option<SystemTime>) -> bool {
    let loaded_at = match loaded_at {
        Some(t) => t,
        None => return false, // never loaded — nothing to invalidate
    };
    let plugins_dir = match workspace_path {
        Some(ws) => ws.join(".diaryx").join("plugins"),
        None => return false,
    };
    let entries = match std::fs::read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let wasm_path = entry.path().join("plugin.wasm");
        if let Ok(meta) = std::fs::metadata(&wasm_path) {
            if let Ok(mtime) = meta.modified() {
                if mtime > loaded_at {
                    log::info!(
                        "[plugins] Detected updated plugin on disk: {}",
                        wasm_path.display()
                    );
                    return true;
                }
            }
        }
    }
    false
}

async fn get_or_init_tauri_diaryx<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Arc<Diaryx<TauriBaseFs>>, SerializableError> {
    let app_state = app.state::<AppState>();

    // Check if plugins were updated on disk by an external process (e.g. CLI).
    {
        let ws_guard = acquire_lock(&app_state.workspace_path)?;
        let loaded_at = *acquire_lock(&app_state.plugins_loaded_at)?;
        if plugins_changed_on_disk(ws_guard.as_deref(), loaded_at) {
            log::info!("[execute] Plugin files changed on disk, invalidating cache");
            let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
            *diaryx_guard = None;
        }
    }

    let cached_diaryx = {
        let diaryx_guard = acquire_lock(&app_state.diaryx)?;
        diaryx_guard.as_ref().map(Arc::clone)
    };

    if let Some(cached) = cached_diaryx {
        log::trace!("[execute] Using cached Diaryx instance");
        return Ok(cached);
    }

    log::debug!("[execute] No cached Diaryx, creating new instance");
    let workspace_path = {
        let ws_guard = acquire_lock(&app_state.workspace_path)?;
        ws_guard.clone()
    };

    let base_fs = SyncToAsyncFs::new(RealFileSystem);
    let mut d = Diaryx::new(base_fs);
    if let Some(ref ws_path) = workspace_path {
        log::debug!("[execute] Setting workspace root: {:?}", ws_path);
        d.set_workspace_root(ws_path.clone());
    }
    #[cfg(feature = "extism-plugins")]
    {
        let adapters = register_extism_plugins(app, &mut d);
        sync_loaded_plugin_adapters(app, adapters);
    }
    let new_diaryx = Arc::new(d);

    let init_failures = new_diaryx.init_plugins().await;
    for (id, err) in &init_failures {
        log::error!("Plugin {} failed to init: {}", id, err);
    }

    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        *diaryx_guard = Some(Arc::clone(&new_diaryx));
        let mut loaded_guard = acquire_lock(&app_state.plugins_loaded_at)?;
        *loaded_guard = Some(SystemTime::now());
        log::debug!("[execute] Cached Diaryx instance for future commands");
    }

    Ok(new_diaryx)
}

// ============================================================================
// Unified Command API
// ============================================================================

/// Execute a command using the unified command pattern.
///
/// This is the primary API for all diaryx operations, replacing the many
/// individual commands with a single entry point.
///
/// ## Example from TypeScript:
/// ```typescript
/// const command = { type: 'GetEntry', params: { path: 'workspace/notes.md' } };
/// const response = await invoke('execute', { commandJson: JSON.stringify(command) });
/// const result = JSON.parse(response);
/// ```
#[tauri::command]
pub async fn execute<R: Runtime>(
    app: AppHandle<R>,
    command_json: String,
) -> Result<String, SerializableError> {
    log::trace!("[execute] Received command");
    log::trace!("[execute] Command JSON: {}", command_json);

    // Parse the command from JSON
    let cmd: Command = serde_json::from_str(&command_json).map_err(|e| {
        log::error!("[execute] Failed to parse command: {}", e);
        SerializableError {
            kind: "ParseError".to_string(),
            message: format!("Failed to parse command JSON: {}", e),
            path: None,
        }
    })?;

    log::trace!(
        "[execute] Parsed command type: {:?}",
        std::mem::discriminant(&cmd)
    );

    // Check if we're in guest mode and get the appropriate filesystem
    // We need to extract data from mutex guards before any async points
    let guest_state = app.state::<GuestModeState>();
    let is_guest = *acquire_lock(&guest_state.active)?;

    // Execute command using appropriate filesystem
    let response = if is_guest {
        // Guest mode: reuse cached Diaryx with persistent in-memory CRDT state
        let diaryx = {
            let diaryx_guard = acquire_lock(&guest_state.diaryx)?;
            diaryx_guard
                .as_ref()
                .cloned()
                .ok_or_else(|| SerializableError {
                    kind: "GuestModeError".to_string(),
                    message: "Guest mode active but not initialized (call start_guest_mode first)"
                        .to_string(),
                    path: None,
                })?
        };
        log::trace!("[execute] Using cached guest Diaryx instance");
        diaryx.execute(cmd).await.map_err(|e| {
            log_execute_error(&e);
            e.to_serializable()
        })?
    } else {
        // Normal mode: use real filesystem
        let diaryx = get_or_init_tauri_diaryx(&app).await?;

        diaryx.execute(cmd).await.map_err(|e| {
            log_execute_error(&e);
            e.to_serializable()
        })?
    };

    // Serialize the response to JSON
    let response_json = serde_json::to_string(&response).map_err(|e| {
        log::error!("[execute] Failed to serialize response: {}", e);
        SerializableError {
            kind: "SerializeError".to_string(),
            message: format!("Failed to serialize response: {}", e),
            path: None,
        }
    })?;

    log::trace!("[execute] Command executed successfully");
    Ok(response_json)
}

// ============================================================================
// Platform Path Resolution
// ============================================================================

/// Get platform-appropriate paths for the app
/// On mobile, user workspace files are rooted in `document_dir` for Files app access,
/// while internal config remains in `app_data_dir`.
/// On desktop, uses the standard dirs crate locations
fn get_platform_paths<R: Runtime>(app: &AppHandle<R>) -> Result<AppPaths, SerializableError> {
    let path_resolver = app.path();

    // Check if we're on mobile (iOS or Android)
    let is_mobile = cfg!(target_os = "ios") || cfg!(target_os = "android");

    if is_mobile {
        // On mobile, use document_dir for user files so they appear in Files app
        let document_dir = path_resolver
            .document_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get document directory: {}", e),
                path: None,
            })?;

        // Use app_data_dir for internal config (not exposed to Files app)
        let data_dir = path_resolver
            .app_data_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get app data directory: {}", e),
                path: None,
            })?;
        let (log_dir, log_file) = logging::log_paths(&data_dir);

        // Workspace goes in Documents so users can access via Files app
        let default_workspace = document_dir.join("Diaryx");
        // Config stays in Application Support (internal)
        let config_path = data_dir.join("config.toml");

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            log_dir,
            log_file,
            is_mobile: true,
            is_apple_build: cfg!(feature = "apple"),
            icloud_workspace: None,
            icloud_active: false,
        })
    } else {
        // On desktop, use standard locations
        let data_dir = path_resolver
            .app_data_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get app data directory: {}", e),
                path: None,
            })?;

        let document_dir = path_resolver
            .document_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get document directory: {}", e),
                path: None,
            })?;
        let (log_dir, log_file) = logging::log_paths(&data_dir);

        // Use the standard config location
        let config_path = path_resolver
            .app_config_dir()
            .map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get config directory: {}", e),
                path: None,
            })?
            .join("config.toml");

        // Default workspace in home directory for desktop
        let default_workspace = if cfg!(all(target_os = "macos", feature = "apple")) {
            data_dir.join("workspace")
        } else {
            path_resolver
                .home_dir()
                .unwrap_or_else(|_| document_dir.clone())
                .join("diaryx")
        };

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            log_dir,
            log_file,
            is_mobile: false,
            is_apple_build: cfg!(feature = "apple"),
            icloud_workspace: None,
            icloud_active: false,
        })
    }
}

fn resolve_workspace_item_path<R: Runtime>(
    app: &AppHandle<R>,
    path: &str,
) -> Result<PathBuf, SerializableError> {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return Ok(candidate);
    }

    let workspace_root = app
        .state::<AppState>()
        .workspace_path
        .lock()
        .map_err(|e| SerializableError {
            kind: "LockError".to_string(),
            message: format!("Failed to acquire lock: mutex is poisoned - {}", e),
            path: None,
        })?
        .clone()
        .ok_or_else(|| SerializableError {
            kind: "WorkspaceNotFound".to_string(),
            message: "No workspace is currently open".to_string(),
            path: None,
        })?;

    Ok(workspace_root.join(path.trim_start_matches('/')))
}

/// Get the app paths for the current platform
#[tauri::command]
pub fn get_app_paths<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    get_platform_paths(&app)
}

/// Read the current native log file so the debug UI can display it inline.
#[tauri::command]
pub fn read_log_file<R: Runtime>(app: AppHandle<R>) -> Result<String, SerializableError> {
    let paths = get_platform_paths(&app)?;
    std::fs::read_to_string(&paths.log_file).map_err(|e| SerializableError {
        kind: "FileRead".to_string(),
        message: format!("Failed to read log file: {}", e),
        path: Some(paths.log_file.clone()),
    })
}

#[tauri::command]
pub fn reveal_in_file_manager<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<(), SerializableError> {
    let _ = (&app, &path);

    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        return Err(SerializableError {
            kind: "UnsupportedPlatform".to_string(),
            message: "Revealing files in the system file manager is not supported on mobile"
                .to_string(),
            path: None,
        });
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        let resolved = resolve_workspace_item_path(&app, &path)?;
        if !resolved.exists() {
            return Err(SerializableError {
                kind: "NotFound".to_string(),
                message: format!("Path not found: {}", resolved.display()),
                path: Some(resolved),
            });
        }

        let resolved = resolved.canonicalize().unwrap_or(resolved);
        app.opener()
            .reveal_item_in_dir(&resolved)
            .map_err(|e| SerializableError {
                kind: "OpenError".to_string(),
                message: format!("Failed to reveal item in file manager: {}", e),
                path: Some(resolved.clone()),
            })?;

        Ok(())
    }
}

/// Read a binary file relative to the workspace root.
/// Used by the attachment system for file reads.
#[tauri::command]
pub fn read_binary_file<R: Runtime>(
    app: AppHandle<R>,
    path: String,
) -> Result<Vec<u8>, SerializableError> {
    let resolved = resolve_workspace_item_path(&app, &path)?;
    std::fs::read(&resolved).map_err(|e| SerializableError {
        kind: "FileRead".to_string(),
        message: format!("Failed to read binary file: {}", e),
        path: Some(resolved),
    })
}

/// Write binary content to a file relative to the workspace root.
/// Used by the attachment system for file uploads.
#[tauri::command]
pub fn write_binary_file<R: Runtime>(
    app: AppHandle<R>,
    path: String,
    data: Vec<u8>,
) -> Result<(), SerializableError> {
    let resolved = resolve_workspace_item_path(&app, &path)?;

    // Ensure parent directory exists
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SerializableError {
            kind: "FileWrite".to_string(),
            message: format!("Failed to create parent directory: {}", e),
            path: Some(resolved.clone()),
        })?;
    }

    std::fs::write(&resolved, &data).map_err(|e| SerializableError {
        kind: "FileWrite".to_string(),
        message: format!("Failed to write binary file: {}", e),
        path: Some(resolved),
    })
}

/// Check whether a direct-distribution desktop update is available.
#[tauri::command]
pub async fn check_for_app_update<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<AppUpdateInfo>, SerializableError> {
    #[cfg(all(
        feature = "desktop-updater",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    {
        let updater = app.updater().map_err(|e| SerializableError {
            kind: "UpdaterError".to_string(),
            message: format!("Failed to initialize updater: {}", e),
            path: None,
        })?;

        let update = updater.check().await.map_err(|e| SerializableError {
            kind: "UpdaterError".to_string(),
            message: format!("Failed to check for app updates: {}", e),
            path: None,
        })?;

        Ok(update.map(|update| AppUpdateInfo {
            version: update.version,
            body: update.body,
        }))
    }

    #[cfg(not(all(
        feature = "desktop-updater",
        not(any(target_os = "android", target_os = "ios"))
    )))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Download, install, and restart into the newest direct-distribution desktop build.
#[tauri::command]
pub async fn install_app_update<R: Runtime>(app: AppHandle<R>) -> Result<bool, SerializableError> {
    #[cfg(all(
        feature = "desktop-updater",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    {
        let updater = app.updater().map_err(|e| SerializableError {
            kind: "UpdaterError".to_string(),
            message: format!("Failed to initialize updater: {}", e),
            path: None,
        })?;

        let Some(update) = updater.check().await.map_err(|e| SerializableError {
            kind: "UpdaterError".to_string(),
            message: format!("Failed to check for app updates: {}", e),
            path: None,
        })?
        else {
            return Ok(false);
        };

        let target_version = update.version.clone();
        log::info!(
            "[install_app_update] Downloading and installing Diaryx {}",
            target_version
        );

        update
            .download_and_install(
                |chunk_length, content_length| {
                    log::debug!(
                        "[install_app_update] Downloaded {} of {:?} bytes",
                        chunk_length,
                        content_length
                    );
                },
                || {
                    log::info!("[install_app_update] Update download finished");
                },
            )
            .await
            .map_err(|e| SerializableError {
                kind: "UpdaterError".to_string(),
                message: format!("Failed to install app update: {}", e),
                path: None,
            })?;

        log::info!(
            "[install_app_update] Restarting into Diaryx {}",
            target_version
        );
        app.restart();

        #[allow(unreachable_code)]
        {
            Ok(true)
        }
    }

    #[cfg(not(all(
        feature = "desktop-updater",
        not(any(target_os = "android", target_os = "ios"))
    )))]
    {
        let _ = app;
        Ok(false)
    }
}

/// Pick a folder using native dialog and set it as workspace
#[tauri::command]
pub async fn pick_workspace_folder<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<AppPaths>, SerializableError> {
    // Suppress unused warning on iOS (app is used on other platforms)
    let _ = &app;
    // Folder picking is not supported on iOS
    #[cfg(target_os = "ios")]
    {
        return Err(SerializableError {
            kind: "UnsupportedPlatform".to_string(),
            message: "Folder picking is not supported on iOS".to_string(),
            path: None,
        });
    }

    #[cfg(not(target_os = "ios"))]
    {
        use tauri_plugin_dialog::DialogExt;

        let paths = get_platform_paths(&app)?;

        // Use folder picker
        let folder_path = app
            .dialog()
            .file()
            .set_title("Select Workspace Folder")
            .blocking_pick_folder();

        let selected_path = match folder_path {
            Some(path) => path.into_path().map_err(|e| SerializableError {
                kind: "PathError".to_string(),
                message: format!("Failed to get folder path: {:?}", e),
                path: None,
            })?,
            None => {
                // User cancelled
                return Ok(None);
            }
        };

        log::info!(
            "[pick_workspace_folder] User selected folder: {:?}",
            selected_path
        );

        // Load existing config or create new one
        let mut config = if paths.config_path.exists() {
            Config::load_from(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
                .await
                .unwrap_or_else(|_| Config::new(paths.default_workspace.clone()))
        } else {
            Config::new(paths.default_workspace.clone())
        };

        // Update workspace path
        let mut config_changed = config.default_workspace != selected_path;
        config.default_workspace = selected_path.clone();

        #[cfg(target_os = "macos")]
        let (actual_workspace, active_access) = {
            config_changed |= store_workspace_bookmark_in_config(&mut config, &selected_path)?;
            let access = activate_workspace_access_from_config(&mut config, &selected_path)?
                .ok_or_else(|| {
                    workspace_access_error(&selected_path, "Missing workspace bookmark")
                })?;
            config_changed |= access.1;
            let actual_workspace = access.0.resolved_path().to_path_buf();
            if config.default_workspace != actual_workspace {
                config.default_workspace = actual_workspace.clone();
                config_changed = true;
            }
            (actual_workspace, Some(access.0))
        };

        #[cfg(not(target_os = "macos"))]
        let actual_workspace = selected_path.clone();

        if config_changed || !paths.config_path.exists() {
            save_config_file(&config, &paths.config_path).await?;
        }

        // Initialize workspace if it doesn't exist
        let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
        let workspace_initialized = match ws.find_root_index_in_dir(&actual_workspace).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => false,
        };

        if !workspace_initialized {
            log::info!(
                "[pick_workspace_folder] Initializing workspace at {:?}",
                actual_workspace
            );
            ws.init_workspace(&actual_workspace, Some("My Workspace"), None)
                .await
                .map_err(|e| e.to_serializable())?;
        }

        {
            let app_state = app.state::<AppState>();
            *acquire_lock(&app_state.workspace_path)? = Some(actual_workspace.clone());
            *acquire_lock(&app_state.diaryx)? = None;
            #[cfg(target_os = "macos")]
            {
                *acquire_lock(&app_state.workspace_access)? = active_access;
            }
        }

        Ok(Some(AppPaths {
            data_dir: paths.data_dir,
            document_dir: paths.document_dir,
            default_workspace: actual_workspace,
            config_path: paths.config_path,
            log_dir: paths.log_dir,
            log_file: paths.log_file,
            is_mobile: paths.is_mobile,
            is_apple_build: paths.is_apple_build,
            icloud_workspace: paths.icloud_workspace.clone(),
            icloud_active: paths.icloud_active,
        }))
    }
}

/// Persist security-scoped access for a workspace path selected by the frontend.
///
/// Shared Tauri UI flows such as "open existing folder" and "relocate
/// workspace" use JS-native folder pickers, so they need an explicit native
/// step to convert the selected path into a persistent bookmark on sandboxed
/// macOS builds.
#[tauri::command]
pub async fn authorize_workspace_path<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: String,
) -> Result<String, SerializableError> {
    let requested_path = PathBuf::from(workspace_path);

    #[cfg(target_os = "macos")]
    {
        let paths = get_platform_paths(&app)?;
        let mut config = load_workspace_config(&paths.config_path, &paths.default_workspace).await;
        let mut config_changed = !paths.config_path.exists()
            || store_workspace_bookmark_in_config(&mut config, &requested_path)?;

        let access = activate_workspace_access_from_config(&mut config, &requested_path)?
            .ok_or_else(|| workspace_access_error(&requested_path, "Missing workspace bookmark"))?;
        config_changed |= access.1;

        if config_changed {
            save_config_file(&config, &paths.config_path).await?;
        }

        let resolved_path = access.0.resolved_path().to_path_buf();
        log::info!(
            "[authorize_workspace_path] Authorized security-scoped workspace access: configured={:?} resolved={:?}",
            requested_path,
            resolved_path
        );
        return Ok(resolved_path.to_string_lossy().into_owned());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(requested_path.to_string_lossy().into_owned())
    }
}

/// Resolve iCloud state from config and set up monitoring if active.
async fn resolve_icloud_state<R: Runtime>(
    _app: &AppHandle<R>,
    config: &Config,
    _actual_workspace: &Path,
    _is_mobile: bool,
) -> (Option<PathBuf>, bool) {
    if !config.icloud_enabled {
        return (None, false);
    }

    #[cfg(feature = "icloud")]
    {
        log::info!("[initialize_app] iCloud is enabled, setting up monitoring...");

        let icloud_workspace = Some(_actual_workspace.to_path_buf());

        // Trigger download for workspace root (no-op on macOS) and start monitoring
        let _ = tauri_plugin_icloud::do_trigger_download(
            _app,
            _actual_workspace.to_string_lossy().into_owned(),
        )
        .await;
        let _ = tauri_plugin_icloud::do_start_monitoring(_app).await;

        (icloud_workspace, true)
    }

    #[cfg(not(feature = "icloud"))]
    {
        (None, false)
    }
}

#[cfg(feature = "icloud")]
const ICLOUD_WORKSPACES_DIR: &str = "Workspaces";
#[cfg(feature = "icloud")]
const ICLOUD_NAMESPACE_PREFIX: &str = "workspace:icloud:";
#[cfg(feature = "icloud")]
const ICLOUD_LOCAL_PREFIX: &str = "builtin.icloud:";

#[cfg(feature = "icloud")]
fn sanitize_workspace_key(raw: &str) -> String {
    let mut key = String::with_capacity(raw.len());
    let mut prev_dash = false;

    for ch in raw.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | ' ') {
            Some('-')
        } else {
            None
        };

        let Some(ch) = normalized else { continue };
        if ch == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
            key.push(ch);
        } else {
            prev_dash = false;
            key.push(ch);
        }
    }

    let key = key.trim_matches('-');
    if key.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        key.to_string()
    }
}

#[cfg(feature = "icloud")]
fn workspace_key_from_remote_id(remote_id: Option<&str>, workspace_name: Option<&str>) -> String {
    let candidate = remote_id.unwrap_or_default().trim();
    if let Some(rest) = candidate.strip_prefix(ICLOUD_NAMESPACE_PREFIX) {
        let trimmed = rest.trim();
        if !trimmed.is_empty() {
            return sanitize_workspace_key(trimmed);
        }
    }
    if let Some(rest) = candidate.strip_prefix(ICLOUD_LOCAL_PREFIX) {
        let trimmed = rest.trim();
        if !trimmed.is_empty() {
            return sanitize_workspace_key(trimmed);
        }
    }
    if !candidate.is_empty() {
        if let Some(file_name) = Path::new(candidate).file_name().and_then(|v| v.to_str()) {
            if !file_name.is_empty() {
                return sanitize_workspace_key(file_name);
            }
        }
        return sanitize_workspace_key(candidate);
    }

    sanitize_workspace_key(workspace_name.unwrap_or("workspace"))
}

#[cfg(feature = "icloud")]
async fn resolve_icloud_workspaces_root<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<PathBuf, SerializableError> {
    let container_info = tauri_plugin_icloud::get_container_url(app)
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to get iCloud container: {}", e),
            path: None,
        })?;

    let root = PathBuf::from(&container_info.documents_url).join(ICLOUD_WORKSPACES_DIR);
    if !root.exists() {
        std::fs::create_dir_all(&root).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create iCloud workspaces directory: {}", e),
            path: Some(root.clone()),
        })?;
    }
    Ok(root)
}

#[cfg(feature = "icloud")]
async fn read_workspace_display_name(workspace_root: &Path) -> Option<String> {
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let root_index = ws
        .find_root_index_in_dir(workspace_root)
        .await
        .ok()
        .flatten()?;
    let content = std::fs::read_to_string(&root_index).ok()?;
    let parsed = frontmatter::parse_or_empty(&content).ok()?;
    frontmatter::get_string(&parsed.frontmatter, "title")
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
}

#[cfg(feature = "icloud")]
async fn list_icloud_workspace_records<R: Runtime>(
    app: &AppHandle<R>,
    config: &Config,
) -> Result<Vec<ICloudWorkspaceRecord>, SerializableError> {
    let root = resolve_icloud_workspaces_root(app).await?;
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let mut records = Vec::new();

    let entries = std::fs::read_dir(&root).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to enumerate iCloud workspaces: {}", e),
        path: Some(root.clone()),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to read iCloud workspace entry: {}", e),
            path: Some(root.clone()),
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let has_root = ws
            .find_root_index_in_dir(&path)
            .await
            .map_err(|e| e.to_serializable())?
            .is_some();
        if !has_root {
            continue;
        }

        let key = entry.file_name().to_string_lossy().into_owned();
        let workspace_name = read_workspace_display_name(&path)
            .await
            .unwrap_or_else(|| key.clone());

        records.push(ICloudWorkspaceRecord {
            workspace_id: format!("{ICLOUD_LOCAL_PREFIX}{key}"),
            workspace_name,
            workspace_path: path.clone(),
            active: config.icloud_enabled && config.default_workspace == path,
        });
    }

    records.sort_by(|a, b| {
        a.workspace_name
            .to_lowercase()
            .cmp(&b.workspace_name.to_lowercase())
    });
    Ok(records)
}

#[cfg(feature = "icloud")]
async fn finalize_icloud_workspace_attach<R: Runtime>(
    app: &AppHandle<R>,
    config: &mut Config,
    paths: &AppPaths,
    workspace_path: PathBuf,
) -> Result<AppPaths, SerializableError> {
    config.icloud_enabled = true;
    config.default_workspace = workspace_path.clone();
    save_config_file(config, &paths.config_path).await?;

    let _ = tauri_plugin_icloud::do_start_monitoring(app).await;

    {
        let app_state = app.state::<AppState>();
        let mut ws_lock = acquire_lock(&app_state.workspace_path)?;
        *ws_lock = Some(workspace_path.clone());
        let mut diaryx_lock = acquire_lock(&app_state.diaryx)?;
        *diaryx_lock = None;
    }

    Ok(AppPaths {
        data_dir: paths.data_dir.clone(),
        document_dir: paths.document_dir.clone(),
        default_workspace: workspace_path.clone(),
        config_path: paths.config_path.clone(),
        log_dir: paths.log_dir.clone(),
        log_file: paths.log_file.clone(),
        is_mobile: paths.is_mobile,
        is_apple_build: paths.is_apple_build,
        icloud_workspace: Some(workspace_path),
        icloud_active: true,
    })
}

/// Initialize the app - creates necessary directories and default workspace if needed
#[tauri::command]
pub async fn initialize_app<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    log::info!("[initialize_app] Starting initialization...");

    let paths = get_platform_paths(&app).map_err(|e| {
        log::error!("[initialize_app] Failed to get platform paths: {:?}", e);
        e
    })?;

    log::info!("[initialize_app] Platform paths resolved:");
    log::info!("  data_dir: {:?}", paths.data_dir);
    log::info!("  document_dir: {:?}", paths.document_dir);
    log::info!("  default_workspace: {:?}", paths.default_workspace);
    log::info!("  config_path: {:?}", paths.config_path);
    log::info!("  log_dir: {:?}", paths.log_dir);
    log::info!("  log_file: {:?}", paths.log_file);
    log::info!("  is_mobile: {}", paths.is_mobile);

    // Create data directory if it doesn't exist
    if !paths.data_dir.exists() {
        log::info!("[initialize_app] Creating data directory...");
        std::fs::create_dir_all(&paths.data_dir).map_err(|e| {
            log::error!("[initialize_app] Failed to create data directory: {}", e);
            SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create data directory: {}", e),
                path: Some(paths.data_dir.clone()),
            }
        })?;
    }

    // Load or create config file FIRST to get the actual workspace path
    log::info!("[initialize_app] Loading/creating config...");
    let config_exists = paths.config_path.exists();
    let mut config = if paths.is_mobile {
        // On mobile, load config if it exists (may have iCloud state), else use defaults
        if paths.config_path.exists() {
            log::info!(
                "[initialize_app] Mobile: loading existing config from {:?}",
                paths.config_path
            );
            Config::load_from(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
                .await
                .unwrap_or_else(|e| {
                    log::warn!(
                        "[initialize_app] Mobile: failed to load config, using defaults: {:?}",
                        e
                    );
                    Config::new(paths.default_workspace.clone())
                })
        } else {
            log::info!(
                "[initialize_app] Mobile: using platform workspace path: {:?}",
                paths.default_workspace
            );
            Config::new(paths.default_workspace.clone())
        }
    } else if paths.config_path.exists() {
        log::info!(
            "[initialize_app] Loading existing config from {:?}",
            paths.config_path
        );
        Config::load_from(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "[initialize_app] Failed to load config, creating new: {:?}",
                    e
                );
                Config::new(paths.default_workspace.clone())
            })
    } else {
        log::info!("[initialize_app] Creating new config with default workspace");
        let new_config = Config::new(paths.default_workspace.clone());
        // Save the new config
        new_config
            .save_to(&SyncToAsyncFs::new(RealFileSystem), &paths.config_path)
            .await
            .map_err(|e| {
                log::error!("[initialize_app] Failed to save config: {:?}", e);
                e.to_serializable()
            })?;
        new_config
    };
    let mut config_changed = !paths.is_mobile && !config_exists;

    // Use the workspace path from config (may differ from platform default)
    let mut actual_workspace = config.default_workspace.clone();

    // On iOS the sandbox container UUID changes between builds/reinstalls, so
    // an absolute path persisted in config may point at a stale container.
    // Re-resolve by extracting just the folder name and joining it to the
    // current document_dir — the same logic reinitialize_workspace uses.
    #[cfg(target_os = "ios")]
    {
        if actual_workspace.is_absolute() {
            let re_resolved = if let Some(name) = actual_workspace.file_name() {
                paths.document_dir.join(name)
            } else {
                paths.default_workspace.clone()
            };
            if re_resolved != actual_workspace {
                log::info!(
                    "[initialize_app] iOS: re-resolved stale workspace path {:?} -> {:?}",
                    actual_workspace,
                    re_resolved
                );
                actual_workspace = re_resolved;
                config.default_workspace = actual_workspace.clone();
                config_changed = true;
            }
        }
    }

    #[cfg(target_os = "macos")]
    let active_access = match activate_workspace_access_from_config(&mut config, &actual_workspace)
    {
        Ok(Some((access, bookmark_changed))) => {
            log::info!(
                "[initialize_app] Restored security-scoped workspace access: configured={:?} resolved={:?}",
                actual_workspace,
                access.resolved_path()
            );
            config_changed |= bookmark_changed;
            actual_workspace = access.resolved_path().to_path_buf();
            if config.default_workspace != actual_workspace {
                config.default_workspace = actual_workspace.clone();
                config_changed = true;
            }
            Some(access)
        }
        Ok(None) => {
            config_changed |=
                try_backfill_workspace_bookmark(&mut config, &actual_workspace, "initialize_app");
            match activate_workspace_access_from_config(&mut config, &actual_workspace) {
                Ok(Some((access, bookmark_changed))) => {
                    log::info!(
                        "[initialize_app] Backfilled security-scoped workspace access: configured={:?} resolved={:?}",
                        actual_workspace,
                        access.resolved_path()
                    );
                    config_changed |= bookmark_changed;
                    actual_workspace = access.resolved_path().to_path_buf();
                    if config.default_workspace != actual_workspace {
                        config.default_workspace = actual_workspace.clone();
                        config_changed = true;
                    }
                    Some(access)
                }
                Ok(None) => {
                    log::info!(
                        "[initialize_app] No stored security-scoped workspace bookmark for {:?}",
                        actual_workspace
                    );
                    None
                }
                Err(e) => {
                    log::warn!(
                        "[initialize_app] Failed to resolve backfilled bookmark for {:?}: {:?} — continuing without sandbox access",
                        actual_workspace,
                        e
                    );
                    None
                }
            }
        }
        Err(e) => {
            log::warn!(
                "[initialize_app] Failed to resolve stored bookmark for {:?}: {:?} — continuing without sandbox access",
                actual_workspace,
                e
            );
            // Stale bookmark — try backfill, but don't abort init
            config_changed |=
                try_backfill_workspace_bookmark(&mut config, &actual_workspace, "initialize_app");
            match activate_workspace_access_from_config(&mut config, &actual_workspace) {
                Ok(Some((access, bookmark_changed))) => {
                    log::info!(
                        "[initialize_app] Backfilled security-scoped workspace access after stale bookmark: {:?}",
                        access.resolved_path()
                    );
                    config_changed |= bookmark_changed;
                    actual_workspace = access.resolved_path().to_path_buf();
                    if config.default_workspace != actual_workspace {
                        config.default_workspace = actual_workspace.clone();
                        config_changed = true;
                    }
                    Some(access)
                }
                _ => {
                    log::info!(
                        "[initialize_app] No valid bookmark available for {:?}, continuing without sandbox access",
                        actual_workspace
                    );
                    None
                }
            }
        }
    };

    if config_changed && !paths.is_mobile {
        save_config_file(&config, &paths.config_path).await?;
    }

    log::info!(
        "[initialize_app] Using workspace from config: {:?}",
        actual_workspace
    );

    // Make sure the workspace directory exists
    if !actual_workspace.exists() {
        log::info!(
            "[initialize_app] Creating workspace directory: {:?}",
            actual_workspace
        );
        std::fs::create_dir_all(&actual_workspace).map_err(|e| {
            log::error!(
                "[initialize_app] Failed to create workspace directory: {}",
                e
            );
            SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create workspace directory: {}", e),
                path: Some(actual_workspace.clone()),
            }
        })?;
    }

    // Check if workspace needs initialization (has a root index file)
    log::info!("[initialize_app] Checking if workspace is initialized...");
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let workspace_has_root = match ws.find_root_index_in_dir(&actual_workspace).await {
        Ok(Some(path)) => {
            log::info!("[initialize_app] Found root index at: {:?}", path);
            true
        }
        Ok(None) => {
            log::info!("[initialize_app] No root index found, workspace needs initialization");
            false
        }
        Err(e) => {
            log::warn!(
                "[initialize_app] Error checking for root index: {:?}, assuming not initialized",
                e
            );
            false
        }
    };

    if !workspace_has_root {
        log::info!(
            "[initialize_app] Initializing workspace at {:?}",
            actual_workspace
        );
        ws.init_workspace(&actual_workspace, Some("My Workspace"), None)
            .await
            .map_err(|e| {
                log::error!("[initialize_app] Failed to initialize workspace: {:?}", e);
                e.to_serializable()
            })?;
        log::info!("[initialize_app] Workspace initialized successfully");
    }

    log::info!("[initialize_app] Initialization complete!");

    // Store workspace path in AppState
    {
        let app_state = app.state::<AppState>();
        let mut ws_lock = acquire_lock(&app_state.workspace_path)?;
        *ws_lock = Some(actual_workspace.clone());
        // Force re-creation of Diaryx on next execute()
        let mut diaryx_lock = acquire_lock(&app_state.diaryx)?;
        *diaryx_lock = None;
        #[cfg(target_os = "macos")]
        {
            *acquire_lock(&app_state.workspace_access)? = active_access;
        }
    }

    // Determine iCloud state from config
    let (icloud_workspace, icloud_active) =
        resolve_icloud_state(&app, &config, &actual_workspace, paths.is_mobile).await;

    // Return paths with the actual workspace from config
    Ok(AppPaths {
        data_dir: paths.data_dir,
        document_dir: paths.document_dir,
        default_workspace: actual_workspace,
        config_path: paths.config_path,
        log_dir: paths.log_dir,
        log_file: paths.log_file,
        is_mobile: paths.is_mobile,
        is_apple_build: paths.is_apple_build,
        icloud_workspace,
        icloud_active,
    })
}

// ============================================================================
// iCloud Commands
// ============================================================================

/// Enable or disable iCloud Drive storage for the workspace.
///
/// When enabling: checks iCloud availability, gets the container URL,
/// migrates files to iCloud, and updates the config.
/// When disabling: migrates files back to local Documents and updates config.
#[cfg(feature = "icloud")]
#[tauri::command]
pub async fn set_icloud_enabled<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<AppPaths, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let mut config =
        Config::load_from_or_default(&fs, &paths.config_path, paths.default_workspace.clone())
            .await;

    if enabled {
        // Check iCloud availability
        let availability = tauri_plugin_icloud::check_available(&app)
            .await
            .map_err(|e| SerializableError {
                kind: "ICloudError".to_string(),
                message: format!("Failed to check iCloud availability: {}", e),
                path: None,
            })?;

        if !availability.is_available {
            return Err(SerializableError {
                kind: "ICloudError".to_string(),
                message: "iCloud is not available. Please sign in to iCloud in Settings.".into(),
                path: None,
            });
        }

        // Get iCloud container URL
        let container_info = tauri_plugin_icloud::get_container_url(&app)
            .await
            .map_err(|e| SerializableError {
                kind: "ICloudError".to_string(),
                message: format!("Failed to get iCloud container: {}", e),
                path: None,
            })?;

        let current_workspace = config.default_workspace.clone();
        let preferred_key = current_workspace
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");
        let icloud_workspace = PathBuf::from(&container_info.documents_url)
            .join(ICLOUD_WORKSPACES_DIR)
            .join(sanitize_workspace_key(preferred_key));

        // Migrate files to iCloud (only if current workspace has content)
        if current_workspace.exists() && current_workspace != icloud_workspace {
            tauri_plugin_icloud::do_migrate_to_icloud(
                &app,
                current_workspace.to_string_lossy().into_owned(),
                icloud_workspace.to_string_lossy().into_owned(),
            )
            .await
            .map_err(|e| SerializableError {
                kind: "ICloudError".to_string(),
                message: format!("Failed to migrate to iCloud: {}", e),
                path: None,
            })?;

            log::info!(
                "[set_icloud_enabled] Migrated files from {:?} to {:?}",
                current_workspace,
                icloud_workspace
            );
        }

        // Update config
        config.icloud_enabled = true;
        config.default_workspace = icloud_workspace.clone();
        save_config_file(&config, &paths.config_path).await?;

        // Start monitoring sync status
        let _ = tauri_plugin_icloud::do_start_monitoring(&app).await;

        // Reinitialize workspace at new path
        let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
        if !icloud_workspace.exists() {
            std::fs::create_dir_all(&icloud_workspace).map_err(|e| SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create iCloud workspace directory: {}", e),
                path: Some(icloud_workspace.clone()),
            })?;
        }
        let workspace_has_root = ws
            .find_root_index_in_dir(&icloud_workspace)
            .await
            .ok()
            .flatten()
            .is_some();
        if !workspace_has_root {
            ws.init_workspace(&icloud_workspace, Some("My Workspace"), None)
                .await
                .map_err(|e| e.to_serializable())?;
        }

        // Update AppState
        {
            let app_state = app.state::<AppState>();
            let mut ws_lock = acquire_lock(&app_state.workspace_path)?;
            *ws_lock = Some(icloud_workspace.clone());
            let mut diaryx_lock = acquire_lock(&app_state.diaryx)?;
            *diaryx_lock = None;
        }

        Ok(AppPaths {
            data_dir: paths.data_dir,
            document_dir: paths.document_dir,
            default_workspace: icloud_workspace.clone(),
            config_path: paths.config_path,
            log_dir: paths.log_dir,
            log_file: paths.log_file,
            is_mobile: paths.is_mobile,
            is_apple_build: paths.is_apple_build,
            icloud_workspace: Some(icloud_workspace),
            icloud_active: true,
        })
    } else {
        // Disabling iCloud — migrate back to local
        let local_workspace = if paths.is_mobile {
            paths.document_dir.join("Diaryx")
        } else {
            // On macOS desktop, use the platform default workspace path
            paths.default_workspace.clone()
        };
        let current_workspace = config.default_workspace.clone();

        if current_workspace.exists() && current_workspace != local_workspace {
            tauri_plugin_icloud::do_migrate_from_icloud(
                &app,
                current_workspace.to_string_lossy().into_owned(),
                local_workspace.to_string_lossy().into_owned(),
            )
            .await
            .map_err(|e| SerializableError {
                kind: "ICloudError".to_string(),
                message: format!("Failed to migrate from iCloud: {}", e),
                path: None,
            })?;

            log::info!(
                "[set_icloud_enabled] Migrated files from {:?} to {:?}",
                current_workspace,
                local_workspace
            );
        }

        // Stop monitoring
        let _ = tauri_plugin_icloud::do_stop_monitoring(&app).await;

        // Update config
        config.icloud_enabled = false;
        config.default_workspace = local_workspace.clone();
        save_config_file(&config, &paths.config_path).await?;

        // Reinitialize workspace at local path
        if !local_workspace.exists() {
            std::fs::create_dir_all(&local_workspace).map_err(|e| SerializableError {
                kind: "IoError".to_string(),
                message: format!("Failed to create local workspace directory: {}", e),
                path: Some(local_workspace.clone()),
            })?;
        }

        // Update AppState
        {
            let app_state = app.state::<AppState>();
            let mut ws_lock = acquire_lock(&app_state.workspace_path)?;
            *ws_lock = Some(local_workspace.clone());
            let mut diaryx_lock = acquire_lock(&app_state.diaryx)?;
            *diaryx_lock = None;
        }

        Ok(AppPaths {
            data_dir: paths.data_dir,
            document_dir: paths.document_dir,
            default_workspace: local_workspace,
            config_path: paths.config_path,
            log_dir: paths.log_dir,
            log_file: paths.log_file,
            is_mobile: paths.is_mobile,
            is_apple_build: paths.is_apple_build,
            icloud_workspace: None,
            icloud_active: false,
        })
    }
}

/// Stub for non-icloud builds.
#[cfg(not(feature = "icloud"))]
#[tauri::command]
pub async fn set_icloud_enabled<R: Runtime>(
    _app: AppHandle<R>,
    _enabled: bool,
) -> Result<AppPaths, SerializableError> {
    Err(SerializableError {
        kind: "ICloudError".to_string(),
        message: "iCloud support is not available in this build".into(),
        path: None,
    })
}

/// Inspect whether the app's iCloud container currently has any Diaryx workspaces.
#[cfg(feature = "icloud")]
#[tauri::command]
pub async fn get_icloud_workspace_info<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ICloudWorkspaceInfo, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let config =
        Config::load_from_or_default(&fs, &paths.config_path, paths.default_workspace.clone())
            .await;

    let availability = tauri_plugin_icloud::check_available(&app)
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to check iCloud availability: {}", e),
            path: None,
        })?;

    if !availability.is_available {
        return Ok(ICloudWorkspaceInfo {
            is_available: false,
            has_workspace: false,
            workspace_path: None,
            workspace_name: None,
            active: false,
        });
    }

    let workspaces = list_icloud_workspace_records(&app, &config).await?;
    let active_workspace = workspaces.iter().find(|workspace| workspace.active);

    Ok(ICloudWorkspaceInfo {
        is_available: true,
        has_workspace: !workspaces.is_empty(),
        workspace_path: active_workspace.map(|workspace| workspace.workspace_path.clone()),
        workspace_name: active_workspace.map(|workspace| workspace.workspace_name.clone()),
        active: active_workspace.is_some(),
    })
}

/// Stub for non-icloud builds.
#[cfg(not(feature = "icloud"))]
#[tauri::command]
pub async fn get_icloud_workspace_info<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<ICloudWorkspaceInfo, SerializableError> {
    Ok(ICloudWorkspaceInfo {
        is_available: false,
        has_workspace: false,
        workspace_path: None,
        workspace_name: None,
        active: false,
    })
}

/// List all Diaryx workspaces stored in the app's iCloud container.
#[cfg(feature = "icloud")]
#[tauri::command]
pub async fn list_icloud_workspaces<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<ICloudWorkspaceRecord>, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let config =
        Config::load_from_or_default(&fs, &paths.config_path, paths.default_workspace.clone())
            .await;

    let availability = tauri_plugin_icloud::check_available(&app)
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to check iCloud availability: {}", e),
            path: None,
        })?;
    if !availability.is_available {
        return Ok(Vec::new());
    }

    list_icloud_workspace_records(&app, &config).await
}

#[cfg(not(feature = "icloud"))]
#[tauri::command]
pub async fn list_icloud_workspaces<R: Runtime>(
    _app: AppHandle<R>,
) -> Result<Vec<ICloudWorkspaceRecord>, SerializableError> {
    Ok(Vec::new())
}

/// Migrate the current workspace into a specific iCloud workspace slot.
#[cfg(feature = "icloud")]
#[tauri::command]
pub async fn link_icloud_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
) -> Result<AppPaths, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let mut config =
        Config::load_from_or_default(&fs, &paths.config_path, paths.default_workspace.clone())
            .await;

    let availability = tauri_plugin_icloud::check_available(&app)
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to check iCloud availability: {}", e),
            path: None,
        })?;
    if !availability.is_available {
        return Err(SerializableError {
            kind: "ICloudError".to_string(),
            message: "iCloud is not available. Please sign in to iCloud in Settings.".into(),
            path: None,
        });
    }

    let root = resolve_icloud_workspaces_root(&app).await?;
    let workspace_key =
        workspace_key_from_remote_id(workspace_id.as_deref(), workspace_name.as_deref());
    let target_workspace = root.join(&workspace_key);
    let current_workspace = config.default_workspace.clone();
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let target_has_root = ws
        .find_root_index_in_dir(&target_workspace)
        .await
        .map_err(|e| e.to_serializable())?
        .is_some();

    if current_workspace != target_workspace && target_has_root {
        return Err(SerializableError {
            kind: "ICloudError".to_string(),
            message: "That iCloud workspace already exists. Restore it instead of linking a new workspace into the same slot.".into(),
            path: Some(target_workspace),
        });
    }

    if current_workspace.exists() && current_workspace != target_workspace {
        tauri_plugin_icloud::do_migrate_to_icloud(
            &app,
            current_workspace.to_string_lossy().into_owned(),
            target_workspace.to_string_lossy().into_owned(),
        )
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to migrate to iCloud: {}", e),
            path: Some(target_workspace.clone()),
        })?;
    }

    if !target_workspace.exists() {
        std::fs::create_dir_all(&target_workspace).map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to create iCloud workspace directory: {}", e),
            path: Some(target_workspace.clone()),
        })?;
    }

    let workspace_has_root = ws
        .find_root_index_in_dir(&target_workspace)
        .await
        .map_err(|e| e.to_serializable())?
        .is_some();
    if !workspace_has_root {
        ws.init_workspace(
            &target_workspace,
            workspace_name.as_deref().or(Some("My Workspace")),
            None,
        )
        .await
        .map_err(|e| e.to_serializable())?;
    }

    finalize_icloud_workspace_attach(&app, &mut config, &paths, target_workspace).await
}

#[cfg(not(feature = "icloud"))]
#[tauri::command]
pub async fn link_icloud_workspace<R: Runtime>(
    _app: AppHandle<R>,
    _workspace_id: Option<String>,
    _workspace_name: Option<String>,
) -> Result<AppPaths, SerializableError> {
    Err(SerializableError {
        kind: "ICloudError".to_string(),
        message: "iCloud support is not available in this build".into(),
        path: None,
    })
}

/// Attach the app to an existing iCloud workspace without migrating local files into it.
#[cfg(feature = "icloud")]
#[tauri::command]
pub async fn restore_icloud_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_id: Option<String>,
) -> Result<AppPaths, SerializableError> {
    let paths = get_platform_paths(&app)?;
    let fs = SyncToAsyncFs::new(RealFileSystem);
    let mut config =
        Config::load_from_or_default(&fs, &paths.config_path, paths.default_workspace.clone())
            .await;

    let availability = tauri_plugin_icloud::check_available(&app)
        .await
        .map_err(|e| SerializableError {
            kind: "ICloudError".to_string(),
            message: format!("Failed to check iCloud availability: {}", e),
            path: None,
        })?;
    if !availability.is_available {
        return Err(SerializableError {
            kind: "ICloudError".to_string(),
            message: "iCloud is not available. Please sign in to iCloud in Settings.".into(),
            path: None,
        });
    }

    let root = resolve_icloud_workspaces_root(&app).await?;
    let workspace_key = workspace_key_from_remote_id(workspace_id.as_deref(), None);
    let icloud_workspace = root.join(&workspace_key);
    let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
    let has_workspace = ws
        .find_root_index_in_dir(&icloud_workspace)
        .await
        .map_err(|e| e.to_serializable())?
        .is_some();
    if !has_workspace {
        return Err(SerializableError {
            kind: "ICloudError".to_string(),
            message: "No existing Diaryx workspace was found in iCloud Drive.".into(),
            path: Some(icloud_workspace.clone()),
        });
    }

    finalize_icloud_workspace_attach(&app, &mut config, &paths, icloud_workspace).await
}

/// Stub for non-icloud builds.
#[cfg(not(feature = "icloud"))]
#[tauri::command]
pub async fn restore_icloud_workspace<R: Runtime>(
    _app: AppHandle<R>,
    _workspace_id: Option<String>,
) -> Result<AppPaths, SerializableError> {
    Err(SerializableError {
        kind: "ICloudError".to_string(),
        message: "iCloud support is not available in this build".into(),
        path: None,
    })
}

// ============================================================================
// Import Commands
// ============================================================================

/// Result of an import operation
#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub success: bool,
    pub files_imported: usize,
    pub files_skipped: usize,
    pub workspace_path: String,
    pub error: Option<String>,
    /// True if user cancelled the file picker
    pub cancelled: bool,
}

/// Import workspace from a backup zip file
#[tauri::command]
pub async fn import_from_zip(
    zip_path: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified and no default workspace configured"
                        .to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    log::info!("[Import] Importing from {} to {:?}", zip_path, workspace);

    // Open zip file
    let zip_file = std::fs::File::open(&zip_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open zip file: {}", e),
        path: Some(PathBuf::from(&zip_path)),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(PathBuf::from(&zip_path)),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown files and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
                || lower.ends_with(".heic")
                || lower.ends_with(".heif")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        // Read and write file
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

/// Pick a zip file using native dialog and import it
#[tauri::command]
pub async fn pick_and_import_zip<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    // Use file picker
    let file_path = app
        .dialog()
        .file()
        .add_filter("Zip Archive", &["zip", "application/zip"])
        .set_title("Select Backup Zip to Import")
        .blocking_pick_file();

    let selected_path = match file_path {
        Some(path) => path.into_path().map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to get file path: {:?}", e),
            path: None,
        })?,
        None => {
            // User cancelled
            return Ok(ImportResult {
                success: false,
                files_imported: 0,
                files_skipped: 0,
                workspace_path: workspace.to_string_lossy().to_string(),
                error: None,
                cancelled: true,
            });
        }
    };

    log::info!(
        "[Import] Importing from {:?} to {:?}",
        selected_path,
        workspace
    );

    // Open and process zip file
    let zip_file = std::fs::File::open(&selected_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open zip file: {}", e),
        path: Some(selected_path.clone()),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(selected_path.clone()),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

/// Import workspace from base64-encoded zip data
#[tauri::command]
pub async fn import_from_zip_data(
    zip_data: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use base64::Engine;
    use std::io::{Cursor, Read};

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    log::info!(
        "[Import] Importing from base64 data ({} chars) to {:?}",
        zip_data.len(),
        workspace
    );

    // Decode base64
    let zip_bytes = base64::engine::general_purpose::STANDARD
        .decode(&zip_data)
        .map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to decode base64: {}", e),
            path: None,
        })?;

    log::info!("[Import] Decoded {} bytes of zip data", zip_bytes.len());

    // Create zip archive from bytes
    let cursor = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: None,
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name
            .split('/')
            .any(|part| part.starts_with('.') || part == "Thumbs.db" || part == "desktop.ini");

        if should_skip {
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
                || lower.ends_with(".heic")
                || lower.ends_with(".heif")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            continue;
        }

        let file_path = workspace.join(&file_name);

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to create directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Chunked Import Commands
// ============================================================================

/// Global storage for in-progress uploads
static UPLOAD_SESSIONS: std::sync::LazyLock<Mutex<HashMap<String, std::fs::File>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Start a chunked upload session
#[tauri::command]
pub async fn start_import_upload() -> Result<String, SerializableError> {
    use uuid::Uuid;

    let session_id = Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!("diaryx_import_{}.zip", &session_id));

    log::info!(
        "[Import] Starting chunked upload session: {} -> {:?}",
        session_id,
        temp_path
    );

    let file = std::fs::File::create(&temp_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to create temp file: {}", e),
        path: Some(temp_path),
    })?;

    UPLOAD_SESSIONS
        .lock()
        .unwrap()
        .insert(session_id.clone(), file);

    Ok(session_id)
}

/// Append a chunk of base64-encoded data to an upload session
#[tauri::command]
pub async fn append_import_chunk(
    session_id: String,
    chunk: String,
) -> Result<usize, SerializableError> {
    use base64::Engine;
    use std::io::Write;

    // Decode base64 chunk
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&chunk)
        .map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to decode chunk: {}", e),
            path: None,
        })?;

    let bytes_len = bytes.len();

    // Write to temp file
    let mut sessions = UPLOAD_SESSIONS.lock().unwrap();
    let file = sessions
        .get_mut(&session_id)
        .ok_or_else(|| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Upload session not found: {}", session_id),
            path: None,
        })?;

    file.write_all(&bytes).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to write chunk: {}", e),
        path: None,
    })?;

    Ok(bytes_len)
}

/// Finish a chunked upload and import the zip file
#[tauri::command]
pub async fn finish_import_upload<R: Runtime>(
    _app: AppHandle<R>,
    session_id: String,
    workspace_path: Option<String>,
) -> Result<ImportResult, SerializableError> {
    use std::io::Read;

    let fs = RealFileSystem;

    // Get workspace path
    let workspace = match workspace_path {
        Some(p) => PathBuf::from(p),
        None => {
            let config = Config::default();
            if config.default_workspace.as_os_str().is_empty() {
                return Err(SerializableError {
                    kind: "ImportError".to_string(),
                    message: "No workspace specified".to_string(),
                    path: None,
                });
            }
            config.default_workspace
        }
    };

    // Close the file and remove from sessions
    let temp_path = {
        let mut sessions = UPLOAD_SESSIONS.lock().unwrap();
        sessions.remove(&session_id);
        std::env::temp_dir().join(format!("diaryx_import_{}.zip", &session_id))
    };

    log::info!(
        "[Import] Finishing chunked upload: {} -> {:?}",
        session_id,
        temp_path
    );

    // Open the completed temp file
    let zip_file = std::fs::File::open(&temp_path).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to open temp file: {}", e),
        path: Some(temp_path.clone()),
    })?;

    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| SerializableError {
        kind: "ImportError".to_string(),
        message: format!("Failed to read zip archive: {}", e),
        path: Some(temp_path.clone()),
    })?;

    let total_files = archive.len();
    let mut files_imported = 0;
    let files_skipped = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SerializableError {
            kind: "ImportError".to_string(),
            message: format!("Failed to read zip entry: {}", e),
            path: None,
        })?;

        if file.is_dir() {
            continue;
        }

        let file_name = file.name().to_string();

        // Skip system files
        let should_skip = file_name.split('/').any(|part| {
            part.starts_with('.')
                || part == ".DS_Store"
                || part == ".git"
                || part == "Thumbs.db"
                || part == "desktop.ini"
        });

        if should_skip {
            log::debug!("[Import] Skipping system file: {}", file_name);
            continue;
        }

        // Only import markdown and attachments
        let is_markdown = file_name.ends_with(".md");
        let is_in_attachments =
            file_name.contains("/attachments/") || file_name.contains("/assets/");
        let is_common_attachment = {
            let lower = file_name.to_lowercase();
            lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".gif")
                || lower.ends_with(".svg")
                || lower.ends_with(".pdf")
                || lower.ends_with(".webp")
        };

        if !is_markdown && !is_in_attachments && !is_common_attachment {
            log::debug!("[Import] Skipping non-workspace file: {}", file_name);
            continue;
        }

        let file_path = workspace.join(&file_name);

        log::info!(
            "[Import] Processing zip entry: {} -> {:?}",
            file_name,
            file_path
        );

        // Create parent directories
        if let Some(parent) = file_path.parent()
            && !parent.as_os_str().is_empty()
        {
            let mut current = workspace.clone();
            for component in std::path::Path::new(&file_name)
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .components()
            {
                current = current.join(component);

                if current.exists() && current.is_file() {
                    // Delete file blocking directory creation
                    std::fs::remove_file(&current).map_err(|e| SerializableError {
                        kind: "ImportError".to_string(),
                        message: format!("Failed to remove conflicting file: {}", e),
                        path: Some(current.clone()),
                    })?;
                    log::info!("[Import] Removed conflicting file: {:?}", current);
                }

                if !current.exists() {
                    std::fs::create_dir(&current).map_err(|e| SerializableError {
                        kind: "ImportError".to_string(),
                        message: format!("Failed to create directory: {}", e),
                        path: Some(current.clone()),
                    })?;
                }
            }
        }

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to read file from zip: {}", e),
                path: Some(file_path.clone()),
            })?;

        fs.write_binary(&file_path, &contents)
            .map_err(|e| SerializableError {
                kind: "ImportError".to_string(),
                message: format!("Failed to write file: {}", e),
                path: Some(file_path.clone()),
            })?;

        files_imported += 1;

        if files_imported % 100 == 0 {
            log::info!(
                "[Import] Progress: {}/{} files",
                files_imported,
                total_files
            );
        }
    }

    // Clean up temp file
    if let Err(e) = std::fs::remove_file(&temp_path) {
        log::warn!("[Import] Failed to clean up temp file: {}", e);
    }

    log::info!(
        "[Import] Complete: {} files imported, {} skipped",
        files_imported,
        files_skipped
    );

    Ok(ImportResult {
        success: true,
        files_imported,
        files_skipped,
        workspace_path: workspace.to_string_lossy().to_string(),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Export Commands
// ============================================================================

/// Export result
#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub success: bool,
    pub files_exported: usize,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Export workspace to a zip file using native save dialog
#[tauri::command]
pub async fn export_to_zip<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
) -> Result<ExportResult, SerializableError> {
    use diaryx_core::fs::FileSystem;
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    // Get workspace name for default filename
    let workspace_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let timestamp = chrono::Utc::now().format("%Y-%m-%d");
    let default_filename = format!("{}-{}.zip", workspace_name, timestamp);

    // Show native save dialog
    let save_path = app
        .dialog()
        .file()
        .add_filter("Zip Archive", &["zip"])
        .set_file_name(&default_filename)
        .set_title("Export Workspace to Zip")
        .blocking_save_file();

    let output_path = match save_path {
        Some(path) => path.into_path().map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to get save path: {:?}", e),
            path: None,
        })?,
        None => {
            // User cancelled
            return Ok(ExportResult {
                success: false,
                files_exported: 0,
                output_path: None,
                error: None,
                cancelled: true,
            });
        }
    };

    log::info!(
        "[Export] Exporting workspace {:?} to {:?}",
        workspace,
        output_path
    );

    // Create zip file
    let file = std::fs::File::create(&output_path).map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to create zip file: {}", e),
        path: Some(output_path.clone()),
    })?;

    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let fs = RealFileSystem;

    // Get all files in workspace
    let all_files = fs
        .list_all_files_recursive(&workspace)
        .map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to list files: {}", e),
            path: None,
        })?;

    let mut files_exported = 0;

    for file_path in all_files {
        // Skip hidden files and directories
        let relative_path =
            pathdiff::diff_paths(&file_path, &workspace).unwrap_or_else(|| file_path.clone());

        let should_skip = relative_path
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));

        if should_skip {
            continue;
        }

        let relative_str = relative_path.to_string_lossy().to_string();

        // Read file content
        let content = match fs.read_binary(&file_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[Export] Failed to read {:?}: {}", file_path, e);
                continue;
            }
        };

        // Add to zip
        if let Err(e) = zip.start_file(&relative_str, options) {
            log::warn!("[Export] Failed to start zip entry {}: {}", relative_str, e);
            continue;
        }

        if let Err(e) = zip.write_all(&content) {
            log::warn!("[Export] Failed to write zip entry {}: {}", relative_str, e);
            continue;
        }

        files_exported += 1;
    }

    zip.finish().map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to finalize zip: {}", e),
        path: Some(output_path.clone()),
    })?;

    log::info!("[Export] Complete: {} files exported", files_exported);

    Ok(ExportResult {
        success: true,
        files_exported,
        output_path: Some(output_path.to_string_lossy().to_string()),
        error: None,
        cancelled: false,
    })
}

/// Export workspace to a specific format (DOCX, EPUB, PDF, etc.) via the publish plugin.
///
/// For markdown, writes files as-is. For other formats, delegates conversion
/// to the publish plugin's `ConvertFormat` command (pandoc WASM).
#[tauri::command]
pub async fn export_to_format<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    format: String,
    audience: Option<String>,
) -> Result<ExportResult, SerializableError> {
    use diaryx_core::export::Exporter;
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    const SUPPORTED_FORMATS: &[&str] = &[
        "markdown", "html", "docx", "epub", "pdf", "latex", "odt", "rst",
    ];

    fn format_extension(format: &str) -> &str {
        match format {
            "markdown" => "md",
            "html" => "html",
            "docx" => "docx",
            "epub" => "epub",
            "pdf" => "pdf",
            "latex" => "tex",
            "odt" => "odt",
            "rst" => "rst",
            _ => format,
        }
    }

    // Validate format
    if !SUPPORTED_FORMATS.contains(&format.as_str()) {
        return Err(SerializableError {
            kind: "ExportError".to_string(),
            message: format!(
                "Unsupported format: '{}'. Supported: {}",
                format,
                SUPPORTED_FORMATS.join(", ")
            ),
            path: None,
        });
    }

    let paths = get_platform_paths(&app)?;
    let workspace = workspace_path
        .map(PathBuf::from)
        .unwrap_or(paths.default_workspace);

    // Get workspace name for default filename
    let workspace_name = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let ext = format_extension(&format);
    let default_filename = format!("{}-export.zip", workspace_name);

    // Show native save dialog
    let save_path = app
        .dialog()
        .file()
        .add_filter("Zip Archive", &["zip"])
        .set_file_name(&default_filename)
        .set_title(&format!("Export as {} (ZIP)", ext.to_uppercase()))
        .blocking_save_file();

    let output_path = match save_path {
        Some(path) => path.into_path().map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to get save path: {:?}", e),
            path: None,
        })?,
        None => {
            return Ok(ExportResult {
                success: false,
                files_exported: 0,
                output_path: None,
                error: None,
                cancelled: true,
            });
        }
    };

    // Find workspace root index
    let async_fs = SyncToAsyncFs::new(RealFileSystem);
    let ws = Workspace::new(async_fs.clone());
    let root_index = ws
        .find_root_index_in_dir(&workspace)
        .await
        .map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to find workspace root: {}", e),
            path: None,
        })?
        .ok_or_else(|| SerializableError {
            kind: "ExportError".to_string(),
            message: "No workspace found".to_string(),
            path: None,
        })?;

    // Plan the export
    let exporter = Exporter::new(async_fs);
    let aud = audience.as_deref().unwrap_or("*");
    let tmp_dest = std::env::temp_dir().join(format!("diaryx-export-{}", uuid::Uuid::new_v4()));
    let plan = exporter
        .plan_export(&root_index, aud, &tmp_dest, None)
        .await
        .map_err(|e| SerializableError {
            kind: "ExportError".to_string(),
            message: format!("Failed to plan export: {}", e),
            path: None,
        })?;

    if plan.included.is_empty() {
        return Ok(ExportResult {
            success: true,
            files_exported: 0,
            output_path: Some(output_path.to_string_lossy().to_string()),
            error: Some("No files matched the audience filter".to_string()),
            cancelled: false,
        });
    }

    // Get plugin adapter for format conversion (when not markdown)
    #[cfg(feature = "extism-plugins")]
    let plugin_adapter = if format != "markdown" {
        let adapters = app.state::<PluginAdapters>();
        let guard = adapters.adapters.lock().map_err(|e| SerializableError {
            kind: "LockError".to_string(),
            message: format!("Failed to lock plugin adapters: {}", e),
            path: None,
        })?;
        guard.get("diaryx.publish").cloned()
    } else {
        None
    };

    // Create zip with converted files
    let file = std::fs::File::create(&output_path).map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to create zip file: {}", e),
        path: Some(output_path.clone()),
    })?;

    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let fs = RealFileSystem;
    let mut files_exported = 0;

    for included in &plan.included {
        let content = match fs.read_to_string(&included.source_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "[ExportFormat] Failed to read {:?}: {}",
                    included.source_path,
                    e
                );
                continue;
            }
        };

        let relative_str = included.relative_path.to_string_lossy().to_string();

        if format == "markdown" {
            // Write markdown as-is
            if let Err(e) = zip.start_file(&relative_str, options) {
                log::warn!("[ExportFormat] zip start_file failed: {}", e);
                continue;
            }
            if let Err(e) = zip.write_all(content.as_bytes()) {
                log::warn!("[ExportFormat] zip write failed: {}", e);
                continue;
            }
        } else {
            // Convert via publish plugin (pandoc WASM)
            let new_path = relative_str.replace(".md", &format!(".{}", ext));

            #[cfg(feature = "extism-plugins")]
            let converted: Result<Vec<u8>, String> = if let Some(ref adapter) = plugin_adapter {
                let input = serde_json::json!({
                    "command": "ConvertFormat",
                    "params": {
                        "content": content,
                        "from": "markdown",
                        "to": format,
                    },
                })
                .to_string();

                match adapter.call_guest("handle_command", &input) {
                    Ok(output) => {
                        let response: serde_json::Value =
                            serde_json::from_str(&output).unwrap_or_default();
                        if response.get("success").and_then(|v| v.as_bool()) == Some(true) {
                            let data = response.get("data").cloned().unwrap_or_default();
                            if let Some(binary_b64) = data.get("binary").and_then(|v| v.as_str()) {
                                use base64::Engine;
                                base64::engine::general_purpose::STANDARD
                                    .decode(binary_b64)
                                    .map_err(|e| format!("base64 decode error: {}", e))
                            } else if let Some(text) = data.get("content").and_then(|v| v.as_str())
                            {
                                Ok(text.as_bytes().to_vec())
                            } else {
                                Err("Plugin returned no content".to_string())
                            }
                        } else {
                            let err = response
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown plugin error");
                            Err(err.to_string())
                        }
                    }
                    Err(e) => Err(format!("Plugin call failed: {}", e)),
                }
            } else {
                Err("Publish plugin not loaded".to_string())
            };

            #[cfg(not(feature = "extism-plugins"))]
            let converted: Result<Vec<u8>, String> =
                Err("Format conversion requires the extism-plugins feature".to_string());

            match converted {
                Ok(data) => {
                    if let Err(e) = zip.start_file(&new_path, options) {
                        log::warn!("[ExportFormat] zip start_file failed: {}", e);
                        continue;
                    }
                    if let Err(e) = zip.write_all(&data) {
                        log::warn!("[ExportFormat] zip write failed: {}", e);
                        continue;
                    }
                }
                Err(e) => {
                    log::warn!(
                        "[ExportFormat] conversion failed for {}: {}",
                        relative_str,
                        e
                    );
                    continue;
                }
            }
        }

        files_exported += 1;
    }

    zip.finish().map_err(|e| SerializableError {
        kind: "ExportError".to_string(),
        message: format!("Failed to finalize zip: {}", e),
        path: Some(output_path.clone()),
    })?;

    log::info!(
        "[ExportFormat] Complete: {} files exported as {}",
        files_exported,
        format
    );

    Ok(ExportResult {
        success: true,
        files_exported,
        output_path: Some(output_path.to_string_lossy().to_string()),
        error: None,
        cancelled: false,
    })
}

// ============================================================================
// Guest Mode Commands
// ============================================================================

/// Start guest mode for a share session.
///
/// Creates an in-memory filesystem and CRDT infrastructure for all operations.
/// This allows the user to join a share session without affecting their local
/// workspace files. All synced files and CRDT state are stored in memory only.
#[tauri::command]
pub async fn start_guest_mode<R: Runtime>(
    app: AppHandle<R>,
    join_code: String,
) -> Result<(), SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let mut active = acquire_lock(&guest_state.active)?;

    if *active {
        return Err(SerializableError {
            kind: "GuestModeError".to_string(),
            message: "Already in guest mode".to_string(),
            path: None,
        });
    }

    // Create in-memory filesystem for guest mode
    let mem_fs = InMemoryFileSystem::new();
    let base_fs = SyncToAsyncFs::new(mem_fs.clone());
    let diaryx = Diaryx::new(base_fs);

    *active = true;
    *acquire_lock(&guest_state.filesystem)? = Some(mem_fs);
    *acquire_lock(&guest_state.join_code)? = Some(join_code.clone());
    *acquire_lock(&guest_state.diaryx)? = Some(Arc::new(diaryx));

    log::info!("[guest_mode] Started guest mode for session: {}", join_code);
    Ok(())
}

/// End guest mode and clear in-memory data.
///
/// This clears the in-memory filesystem, CRDT state, and cached Diaryx instance,
/// returning the app to normal mode. All guest session data will be discarded.
#[tauri::command]
pub async fn end_guest_mode<R: Runtime>(app: AppHandle<R>) -> Result<(), SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let mut active = acquire_lock(&guest_state.active)?;

    let was_active = *active;
    let join_code = acquire_lock(&guest_state.join_code)?.clone();

    *active = false;
    *acquire_lock(&guest_state.join_code)? = None;
    *acquire_lock(&guest_state.filesystem)? = None;
    *acquire_lock(&guest_state.diaryx)? = None;

    if was_active {
        log::info!(
            "[guest_mode] Ended guest mode for session: {}",
            join_code.unwrap_or_else(|| "unknown".to_string())
        );
    } else {
        log::debug!("[guest_mode] end_guest_mode called but guest mode was not active");
    }

    Ok(())
}

/// Check if guest mode is currently active.
#[tauri::command]
pub fn is_guest_mode<R: Runtime>(app: AppHandle<R>) -> Result<bool, SerializableError> {
    let guest_state = app.state::<GuestModeState>();
    let active = acquire_lock(&guest_state.active)?;
    Ok(*active)
}

// ============================================================================
// Workspace Reinitialization
// ============================================================================

/// Reinitialize the app for a different workspace directory.
///
/// Used when switching workspaces in Tauri. Clears cached state so
/// the next `execute()` call creates a fresh Diaryx instance.
#[tauri::command]
pub async fn reinitialize_workspace<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: String,
    create: Option<bool>,
) -> Result<AppPaths, SerializableError> {
    let create = create.unwrap_or(false);
    log::info!(
        "[reinitialize_workspace] Reinitializing for workspace: {} (create={})",
        workspace_path,
        create,
    );

    // 1. Clear cached diaryx (forces re-creation on next execute())
    let state = app.state::<AppState>();
    {
        *acquire_lock(&state.diaryx)? = None;
    }

    // 2. Resolve and validate workspace directory
    let paths = get_platform_paths(&app)?;
    let ws_path = PathBuf::from(&workspace_path);

    // On iOS, the sandbox container UUID changes between launches, so stored
    // absolute paths become invalid. Re-resolve by extracting the workspace
    // folder name and joining it to the current document_dir.
    #[cfg(target_os = "ios")]
    let ws_path = {
        if ws_path.is_absolute() {
            if let Some(name) = ws_path.file_name() {
                paths.document_dir.join(name)
            } else {
                paths.default_workspace.clone()
            }
        } else {
            paths.document_dir.join(&ws_path)
        }
    };

    #[cfg(not(target_os = "ios"))]
    let mut ws_path = ws_path;

    #[cfg(target_os = "macos")]
    let active_access = {
        let mut config_changed = false;
        let mut loaded_config =
            load_workspace_config(&paths.config_path, &paths.default_workspace).await;

        match activate_workspace_access_from_config(&mut loaded_config, &ws_path) {
            Ok(Some((access, bookmark_changed))) => {
                log::info!(
                    "[reinitialize_workspace] Restored security-scoped workspace access: configured={:?} resolved={:?}",
                    ws_path,
                    access.resolved_path()
                );
                config_changed |= bookmark_changed;
                ws_path = access.resolved_path().to_path_buf();
                if loaded_config.default_workspace == PathBuf::from(&workspace_path)
                    && loaded_config.default_workspace != ws_path
                {
                    loaded_config.default_workspace = ws_path.clone();
                    config_changed = true;
                }
                if config_changed {
                    save_config_file(&loaded_config, &paths.config_path).await?;
                }
                Some(access)
            }
            Ok(None) => {
                config_changed |= try_backfill_workspace_bookmark(
                    &mut loaded_config,
                    &ws_path,
                    "reinitialize_workspace",
                );
                match activate_workspace_access_from_config(&mut loaded_config, &ws_path) {
                    Ok(Some((access, bookmark_changed))) => {
                        log::info!(
                            "[reinitialize_workspace] Backfilled security-scoped workspace access: configured={:?} resolved={:?}",
                            ws_path,
                            access.resolved_path()
                        );
                        config_changed |= bookmark_changed;
                        ws_path = access.resolved_path().to_path_buf();
                        if loaded_config.default_workspace == PathBuf::from(&workspace_path)
                            && loaded_config.default_workspace != ws_path
                        {
                            loaded_config.default_workspace = ws_path.clone();
                            config_changed = true;
                        }
                        if config_changed {
                            save_config_file(&loaded_config, &paths.config_path).await?;
                        }
                        Some(access)
                    }
                    Ok(None) => {
                        log::info!(
                            "[reinitialize_workspace] No stored security-scoped workspace bookmark for {:?}",
                            ws_path
                        );
                        None
                    }
                    Err(e) => {
                        log::warn!(
                            "[reinitialize_workspace] Failed to resolve backfilled bookmark for {:?}: {:?} — continuing without sandbox access",
                            ws_path,
                            e
                        );
                        None
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[reinitialize_workspace] Failed to resolve stored bookmark for {:?}: {:?} — continuing without sandbox access",
                    ws_path,
                    e
                );
                config_changed |= try_backfill_workspace_bookmark(
                    &mut loaded_config,
                    &ws_path,
                    "reinitialize_workspace",
                );
                match activate_workspace_access_from_config(&mut loaded_config, &ws_path) {
                    Ok(Some((access, bookmark_changed))) => {
                        log::info!(
                            "[reinitialize_workspace] Backfilled after stale bookmark: {:?}",
                            access.resolved_path()
                        );
                        config_changed |= bookmark_changed;
                        ws_path = access.resolved_path().to_path_buf();
                        if config_changed {
                            save_config_file(&loaded_config, &paths.config_path).await?;
                        }
                        Some(access)
                    }
                    _ => None,
                }
            }
        }
    };

    // Check that the workspace directory actually exists. If it was moved or
    // deleted externally we must NOT silently recreate an empty directory —
    // the frontend should surface an error so the user can relocate or remove
    // the stale workspace entry.  Skip this check when `create` is true (new
    // workspace being downloaded/created for the first time).
    if !create && !ws_path.exists() {
        return Err(SerializableError {
            kind: "WorkspaceDirectoryMissing".to_string(),
            message: format!(
                "Workspace directory not found: {}. It may have been moved or deleted.",
                ws_path.display()
            ),
            path: Some(ws_path),
        });
    }

    // Ensure subdirectories exist (e.g. .diaryx metadata folder)
    std::fs::create_dir_all(&ws_path).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to create workspace directory: {}", e),
        path: Some(ws_path.clone()),
    })?;

    // 3. Update AppState
    {
        *acquire_lock(&state.workspace_path)? = Some(ws_path.clone());
        #[cfg(target_os = "macos")]
        {
            *acquire_lock(&state.workspace_access)? = active_access;
        }
    }

    // 4. Return AppPaths
    Ok(AppPaths {
        data_dir: paths.data_dir,
        document_dir: paths.document_dir,
        default_workspace: ws_path,
        config_path: paths.config_path,
        log_dir: paths.log_dir,
        log_file: paths.log_file,
        is_mobile: paths.is_mobile,
        is_apple_build: paths.is_apple_build,
        icloud_workspace: paths.icloud_workspace.clone(),
        icloud_active: paths.icloud_active,
    })
}

// ============================================================================
// HTTP Proxy (iOS CORS bypass)
// ============================================================================

/// Response from the proxy_fetch command.
#[derive(Debug, Serialize)]
pub struct ProxyFetchResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body_base64: String,
}

/// Make an HTTP request natively, bypassing CORS restrictions.
///
/// On iOS, WKWebView enforces CORS and blocks requests from tauri://localhost
/// to external origins. This command uses reqwest to make the request natively.
#[tauri::command]
pub async fn proxy_fetch(
    url: String,
    method: String,
    headers: HashMap<String, String>,
    body_base64: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<ProxyFetchResponse, SerializableError> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    let client = reqwest::Client::new();

    let req_method =
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| SerializableError {
            kind: "RequestError".to_string(),
            message: format!("Invalid HTTP method '{}': {}", method, e),
            path: None,
        })?;

    let mut builder = client.request(req_method, &url);

    // Set timeout
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(std::time::Duration::from_millis(ms));
    }

    // Set headers
    let mut header_map = HeaderMap::new();
    for (key, value) in &headers {
        let name = HeaderName::from_bytes(key.as_bytes()).map_err(|e| SerializableError {
            kind: "RequestError".to_string(),
            message: format!("Invalid header name '{}': {}", key, e),
            path: None,
        })?;
        let val = HeaderValue::from_str(value).map_err(|e| SerializableError {
            kind: "RequestError".to_string(),
            message: format!("Invalid header value for '{}': {}", key, e),
            path: None,
        })?;
        header_map.insert(name, val);
    }
    builder = builder.headers(header_map);

    // Set body
    if let Some(b64) = body_base64 {
        let body_bytes = STANDARD.decode(&b64).map_err(|e| SerializableError {
            kind: "RequestError".to_string(),
            message: format!("Invalid base64 body: {}", e),
            path: None,
        })?;
        builder = builder.body(body_bytes);
    }

    // Send request
    let response = builder.send().await.map_err(|e| {
        let kind = if e.is_timeout() {
            "TimeoutError"
        } else if e.is_connect() {
            "ConnectionError"
        } else {
            "NetworkError"
        };
        SerializableError {
            kind: kind.to_string(),
            message: format!("{}", e),
            path: None,
        }
    })?;

    // Read response
    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();

    let mut resp_headers = HashMap::new();
    for (name, value) in response.headers() {
        if let Ok(v) = value.to_str() {
            resp_headers.insert(name.as_str().to_string(), v.to_string());
        }
    }

    let body_bytes = response.bytes().await.map_err(|e| SerializableError {
        kind: "NetworkError".to_string(),
        message: format!("Failed to read response body: {}", e),
        path: None,
    })?;

    let body_base64 = STANDARD.encode(&body_bytes);

    Ok(ProxyFetchResponse {
        status,
        status_text,
        headers: resp_headers,
        body_base64,
    })
}

// ============================================================================
// OAuth Webview (Native OAuth popup for Tauri)
// ============================================================================

/// Percent-decode a URL query parameter value.
#[cfg(not(target_os = "ios"))]
fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(result).unwrap_or_else(|_| s.to_string())
}

/// Open a webview window for OAuth sign-in and intercept the redirect to
/// extract the authorization code.
///
/// On desktop, this opens a secondary Tauri window that navigates to the
/// OAuth URL. When the OAuth provider redirects back to `redirect_prefix`,
/// the code is extracted from the query string and returned.
#[tauri::command]
pub async fn oauth_webview<R: Runtime>(
    app: AppHandle<R>,
    url: String,
    redirect_prefix: String,
) -> Result<serde_json::Value, SerializableError> {
    // Suppress unused warnings on iOS
    let _ = (&app, &url, &redirect_prefix);

    #[cfg(target_os = "ios")]
    {
        return Err(SerializableError {
            kind: "UnsupportedPlatform".to_string(),
            message:
                "OAuth webview is not supported on iOS — use ASWebAuthenticationSession instead"
                    .to_string(),
            path: None,
        });
    }

    #[cfg(not(target_os = "ios"))]
    {
        use tauri::{WebviewUrl, WebviewWindowBuilder};
        use tokio::sync::oneshot;

        let (tx, rx) = oneshot::channel::<String>();
        let tx = std::sync::Mutex::new(Some(tx));
        let prefix = redirect_prefix.clone();

        let window = WebviewWindowBuilder::new(
            &app,
            "oauth",
            WebviewUrl::External(
                url.parse()
                    .map_err(|e: url::ParseError| SerializableError {
                        kind: "OAuthError".to_string(),
                        message: format!("Invalid OAuth URL: {e}"),
                        path: None,
                    })?,
            ),
        )
        .title("Sign in")
        .inner_size(500.0, 600.0)
        .on_navigation(move |nav_url| {
            let url_str = nav_url.as_str();
            if url_str.starts_with(&prefix) {
                if let Some(query) = nav_url.query() {
                    for pair in query.split('&') {
                        if let Some(val) = pair.strip_prefix("code=") {
                            let code = percent_decode(val);
                            if let Ok(mut guard) = tx.lock() {
                                if let Some(sender) = guard.take() {
                                    let _ = sender.send(code);
                                }
                            }
                            break;
                        }
                    }
                }
                return false; // Block navigation — we have the code
            }
            true
        })
        .build()
        .map_err(|e| SerializableError {
            kind: "OAuthError".to_string(),
            message: format!("Failed to create OAuth window: {e}"),
            path: None,
        })?;

        // Wait for the code or window close
        let code = rx.await.map_err(|_| SerializableError {
            kind: "OAuthError".to_string(),
            message: "OAuth window closed without completing sign-in".to_string(),
            path: None,
        })?;

        let _ = window.close();
        Ok(serde_json::json!({ "code": code }))
    }
}

#[cfg(all(test, feature = "extism-plugins"))]
mod tests {
    use super::*;

    fn rule(include: &[&str], exclude: &[&str]) -> PermissionRule {
        PermissionRule {
            include: include.iter().map(|value| (*value).to_string()).collect(),
            exclude: exclude.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn merge_requested_permission_defaults_backfills_missing_rules() {
        let mut plugins = HashMap::new();
        let defaults = PluginPermissions {
            read_files: Some(rule(&["all"], &[])),
            http_requests: Some(rule(&["sync.diaryx.app"], &[])),
            plugin_storage: Some(rule(&["all"], &[])),
            ..PluginPermissions::default()
        };

        let changed = merge_requested_permission_defaults(&mut plugins, "diaryx.sync", &defaults);

        assert!(changed);
        let permissions = &plugins["diaryx.sync"].permissions;
        assert_eq!(
            permissions
                .read_files
                .as_ref()
                .expect("read_files should be set")
                .include,
            vec!["all".to_string()]
        );
        assert_eq!(
            permissions
                .http_requests
                .as_ref()
                .expect("http_requests should be set")
                .include,
            vec!["sync.diaryx.app".to_string()]
        );
        assert_eq!(
            permissions
                .plugin_storage
                .as_ref()
                .expect("plugin_storage should be set")
                .include,
            vec!["all".to_string()]
        );
    }

    #[test]
    fn merge_requested_permission_defaults_preserves_existing_non_empty_rules() {
        let mut plugins = HashMap::from([(
            "diaryx.sync".to_string(),
            PluginConfig {
                download: Some("https://example.com/plugin.wasm".to_string()),
                permissions: PluginPermissions {
                    read_files: Some(rule(&["journal/daily"], &[])),
                    http_requests: Some(rule(&["sync.diaryx.app"], &[])),
                    ..PluginPermissions::default()
                },
            },
        )]);
        let defaults = PluginPermissions {
            read_files: Some(rule(&["all"], &[])),
            http_requests: Some(rule(&["all"], &[])),
            plugin_storage: Some(rule(&["all"], &[])),
            ..PluginPermissions::default()
        };

        let changed = merge_requested_permission_defaults(&mut plugins, "diaryx.sync", &defaults);

        assert!(changed, "missing plugin_storage should still be backfilled");
        let config = &plugins["diaryx.sync"];
        assert_eq!(
            config.download.as_deref(),
            Some("https://example.com/plugin.wasm")
        );
        assert_eq!(
            config
                .permissions
                .read_files
                .as_ref()
                .expect("existing read_files should be preserved")
                .include,
            vec!["journal/daily".to_string()]
        );
        assert_eq!(
            config
                .permissions
                .http_requests
                .as_ref()
                .expect("existing http_requests should be preserved")
                .include,
            vec!["sync.diaryx.app".to_string()]
        );
        assert_eq!(
            config
                .permissions
                .plugin_storage
                .as_ref()
                .expect("plugin_storage should be backfilled")
                .include,
            vec!["all".to_string()]
        );
    }

    #[test]
    fn merge_requested_permission_defaults_replaces_empty_rules() {
        let mut plugins = HashMap::from([(
            "diaryx.sync".to_string(),
            PluginConfig {
                download: None,
                permissions: PluginPermissions {
                    read_files: Some(rule(&[], &[])),
                    ..PluginPermissions::default()
                },
            },
        )]);
        let defaults = PluginPermissions {
            read_files: Some(rule(&["all"], &[])),
            ..PluginPermissions::default()
        };

        let changed = merge_requested_permission_defaults(&mut plugins, "diaryx.sync", &defaults);

        assert!(changed);
        assert_eq!(
            plugins["diaryx.sync"]
                .permissions
                .read_files
                .as_ref()
                .expect("empty rule should be replaced")
                .include,
            vec!["all".to_string()]
        );
    }

    #[test]
    fn make_permission_checker_allows_rootless_workspace_bootstrap() {
        let root = std::env::temp_dir().join(format!(
            "diaryx-rootless-bootstrap-{}",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("test workspace directory should be created");

        let checker = make_permission_checker(Some(root.clone()));
        let result =
            checker.check_permission("diaryx.sync", PermissionType::CreateFiles, "README.md");

        assert!(
            result.is_ok(),
            "rootless workspace bootstrap should allow provider writes: {result:?}"
        );

        std::fs::write(
            root.join("README.md"),
            "---\ntitle: Restored\ncontents: []\n---\n\n# Restored\n",
        )
        .expect("root index should be written");
        let after_root_index =
            checker.check_permission("diaryx.sync", PermissionType::CreateFiles, "after-root.md");

        let _ = std::fs::remove_dir_all(&root);
        assert!(
            after_root_index.is_err(),
            "frontmatter permissions should apply once the root index exists"
        );
    }
}
