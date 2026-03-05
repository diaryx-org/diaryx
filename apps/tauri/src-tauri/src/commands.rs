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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use diaryx_core::{
    Command,
    config::Config,
    diaryx::Diaryx,
    error::SerializableError,
    fs::{FileSystem, InMemoryFileSystem, RealFileSystem, SyncToAsyncFs},
    workspace::Workspace,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

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
    /// Whether this is a mobile platform (iOS/Android)
    pub is_mobile: bool,
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
}

impl AppState {
    pub fn new() -> Self {
        Self {
            workspace_path: Mutex::new(None),
            diaryx: Mutex::new(None),
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

#[cfg(feature = "extism-plugins")]
fn make_permission_checker(
    workspace_root: Option<PathBuf>,
) -> Arc<dyn diaryx_extism::PermissionChecker> {
    Arc::new(diaryx_extism::FrontmatterPermissionChecker::from_workspace_root(workspace_root))
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
fn register_extism_plugins<FS: diaryx_core::fs::AsyncFileSystem + 'static>(
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

    // Use a basic real filesystem for host function file access.
    let fs: Arc<dyn diaryx_core::fs::AsyncFileSystem> =
        Arc::new(SyncToAsyncFs::new(RealFileSystem));
    let host_ctx = Arc::new(diaryx_extism::HostContext {
        fs,
        storage: Arc::new(diaryx_extism::NoopStorage),
        event_emitter: Arc::new(diaryx_extism::NoopEventEmitter),
        plugin_id: String::new(),
        permission_checker: Some(make_permission_checker(diaryx.workspace_root())),
        file_provider: Arc::new(diaryx_extism::NoopFileProvider),
    });
    let mut adapters = Vec::new();
    match diaryx_extism::load_plugins_from_dir(&plugins_dir, host_ctx) {
        Ok(plugins) => {
            for plugin in plugins {
                let arc = Arc::new(plugin);
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
// Extism Sync Plugin Loading
// ============================================================================

/// Tauri event emitter for the Extism sync plugin.
///
/// Forwards `host_emit_event` calls from the guest plugin to the Tauri frontend
/// via `AppHandle::emit()`.
#[cfg(feature = "extism-plugins")]
struct TauriEventEmitter<R: Runtime> {
    app: AppHandle<R>,
}

#[cfg(feature = "extism-plugins")]
impl<R: Runtime> diaryx_extism::EventEmitter for TauriEventEmitter<R> {
    fn emit(&self, event_json: &str) {
        // Try to parse the event to route to the right Tauri event name.
        // Falls back to a generic "sync-plugin-event" if parsing fails.
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(event_json) {
            let event_type = event
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown");

            match event_type {
                "status_changed" => {
                    let _ = self.app.emit("sync-status-changed", event_json);
                }
                "files_changed" => {
                    let _ = self.app.emit("sync-files-changed", event_json);
                }
                "body_changed" => {
                    let _ = self.app.emit("sync-body-changed", event_json);
                }
                _ => {
                    let _ = self.app.emit("sync-plugin-event", event_json);
                }
            }
        } else {
            let _ = self.app.emit("sync-plugin-event", event_json);
        }
    }
}

/// Holds loaded [`ExtismPluginAdapter`] instances by plugin ID for render IPC calls.
///
/// The frontend can call `call_plugin_render` to invoke a plugin's render export
/// (e.g., math rendering) without needing browser Extism support.
#[cfg(feature = "extism-plugins")]
pub struct PluginAdapters {
    pub adapters: Mutex<HashMap<String, Arc<diaryx_extism::ExtismPluginAdapter>>>,
}

#[cfg(feature = "extism-plugins")]
impl PluginAdapters {
    pub fn new() -> Self {
        Self {
            adapters: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(feature = "extism-plugins")]
impl Default for PluginAdapters {
    fn default() -> Self {
        Self::new()
    }
}

/// State for the Extism sync plugin (loaded on demand).
///
/// Holds the loaded [`ExtismPluginAdapter`] so it can be used for sync operations
/// (binary message handling, drain, etc.) and registered as a WorkspacePlugin.
#[cfg(feature = "extism-plugins")]
pub struct ExtismSyncState {
    /// The loaded Extism sync plugin adapter.
    pub plugin: Mutex<Option<Arc<diaryx_extism::ExtismPluginAdapter>>>,
}

#[cfg(feature = "extism-plugins")]
impl ExtismSyncState {
    pub fn new() -> Self {
        Self {
            plugin: Mutex::new(None),
        }
    }
}

#[cfg(feature = "extism-plugins")]
impl Default for ExtismSyncState {
    fn default() -> Self {
        Self::new()
    }
}

/// Load the Extism sync plugin from the bundled WASM file.
///
/// Creates a [`diaryx_extism::HostContext`] with:
/// - Filesystem: real filesystem via `SyncToAsyncFs<RealFileSystem>`
/// - Storage: SQLite-backed `SqlitePluginStorage` from the active CRDT storage
/// - Events: `TauriEventEmitter` forwarding to the Tauri frontend
///
/// The loaded plugin is registered as a WorkspacePlugin on the cached Diaryx
/// instance, and stored in `ExtismSyncState` for binary sync operations.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn load_sync_plugin<R: Runtime>(
    app: AppHandle<R>,
    wasm_path: Option<String>,
) -> Result<(), SerializableError> {
    log::info!("[load_sync_plugin] Loading Extism sync plugin");

    // Determine WASM path: explicit, bundled, or default location
    let wasm_file = if let Some(path) = wasm_path {
        PathBuf::from(path)
    } else {
        // Check bundled location first, then workspace-local plugins dir
        let bundled = app
            .path()
            .resource_dir()
            .ok()
            .map(|d| d.join("plugins").join("diaryx_sync.wasm"));
        let workspace_local =
            workspace_plugins_dir(&app).map(|d| d.join("diaryx_sync").join("plugin.wasm"));

        bundled
            .filter(|p| p.exists())
            .or_else(|| workspace_local.filter(|p| p.exists()))
            .unwrap_or_else(|| PathBuf::from("diaryx_sync.wasm"))
    };

    if !wasm_file.exists() {
        return Err(SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Sync plugin WASM not found at {}", wasm_file.display()),
            path: None,
        });
    }

    log::info!("[load_sync_plugin] Loading from {}", wasm_file.display());

    // Build HostContext with event emitter (plugin manages its own state)
    let event_emitter: Arc<dyn diaryx_extism::EventEmitter> =
        Arc::new(TauriEventEmitter { app: app.clone() });

    let fs: Arc<dyn diaryx_core::fs::AsyncFileSystem> =
        Arc::new(SyncToAsyncFs::new(RealFileSystem));

    let workspace_root = app
        .try_state::<AppState>()
        .and_then(|state| state.workspace_path.lock().ok().and_then(|v| (*v).clone()));

    let host_ctx = Arc::new(diaryx_extism::HostContext {
        fs,
        storage: Arc::new(diaryx_extism::NoopStorage),
        event_emitter,
        plugin_id: String::new(),
        permission_checker: Some(make_permission_checker(workspace_root)),
        file_provider: Arc::new(diaryx_extism::NoopFileProvider),
    });

    // Load the plugin
    let adapter =
        diaryx_extism::load_plugin_from_wasm(&wasm_file, host_ctx, None).map_err(|e| {
            SerializableError {
                kind: "PluginError".to_string(),
                message: format!("Failed to load sync plugin: {e}"),
                path: None,
            }
        })?;

    let adapter = Arc::new(adapter);

    // Store in ExtismSyncState
    let extism_sync_state = app.state::<ExtismSyncState>();
    {
        let mut plugin_guard = acquire_lock(&extism_sync_state.plugin)?;
        *plugin_guard = Some(Arc::clone(&adapter));
    }

    // Clear cached Diaryx instance so next execute() picks up the plugin
    let app_state = app.state::<AppState>();
    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        *diaryx_guard = None;
    }

    {
        use diaryx_core::plugin::Plugin;
        log::info!(
            "[load_sync_plugin] Sync plugin loaded: {} ({})",
            adapter.manifest().name,
            adapter.manifest().id,
        );
    }

    Ok(())
}

/// Unload the Extism sync plugin.
#[cfg(feature = "extism-plugins")]
#[tauri::command]
pub async fn unload_sync_plugin<R: Runtime>(app: AppHandle<R>) -> Result<(), SerializableError> {
    log::info!("[unload_sync_plugin] Unloading Extism sync plugin");

    let extism_sync_state = app.state::<ExtismSyncState>();
    {
        let mut plugin_guard = acquire_lock(&extism_sync_state.plugin)?;
        *plugin_guard = None;
    }

    // Clear cached Diaryx to remove the registered plugin
    let app_state = app.state::<AppState>();
    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        *diaryx_guard = None;
    }

    log::info!("[unload_sync_plugin] Sync plugin unloaded");
    Ok(())
}

/// Stub: load_sync_plugin when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn load_sync_plugin<R: Runtime>(
    _app: AppHandle<R>,
    _wasm_path: Option<String>,
) -> Result<(), SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

/// Stub: unload_sync_plugin when extism-plugins feature is disabled.
#[cfg(not(feature = "extism-plugins"))]
#[tauri::command]
pub async fn unload_sync_plugin<R: Runtime>(_app: AppHandle<R>) -> Result<(), SerializableError> {
    Err(SerializableError {
        kind: "Unsupported".to_string(),
        message: "Extism plugin support is not enabled. Build with --features extism-plugins."
            .to_string(),
        path: None,
    })
}

// ============================================================================
// User Plugin Install/Uninstall
// ============================================================================

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

    // Load to extract the manifest.
    let fs: Arc<dyn diaryx_core::fs::AsyncFileSystem> =
        Arc::new(SyncToAsyncFs::new(RealFileSystem));
    let host_ctx = Arc::new(diaryx_extism::HostContext {
        fs,
        storage: Arc::new(diaryx_extism::NoopStorage),
        event_emitter: Arc::new(diaryx_extism::NoopEventEmitter),
        plugin_id: String::new(),
        permission_checker: Some(Arc::new(diaryx_extism::DenyAllPermissionChecker)),
        file_provider: Arc::new(diaryx_extism::NoopFileProvider),
    });

    let adapter = diaryx_extism::load_plugin_from_wasm(&tmp_wasm, host_ctx, None).map_err(|e| {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        SerializableError {
            kind: "PluginError".to_string(),
            message: format!("Invalid WASM plugin: {e}"),
            path: None,
        }
    })?;

    let manifest = adapter.manifest();
    let plugin_id = manifest.id.0.clone();
    let manifest_json = serde_json::to_string(&manifest).map_err(|e| SerializableError {
        kind: "SerializationError".to_string(),
        message: format!("Failed to serialize manifest: {e}"),
        path: None,
    })?;

    // Persist WASM to {workspace_root}/.diaryx/plugins/{plugin_id}/plugin.wasm
    let base_dir = workspace_plugins_dir(&app).ok_or_else(|| SerializableError {
        kind: "NotFound".to_string(),
        message: "No workspace is open — cannot install plugin".to_string(),
        path: None,
    })?;
    let plugins_dir = base_dir.join(&plugin_id);
    std::fs::create_dir_all(&plugins_dir).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to create plugin directory: {e}"),
        path: Some(plugins_dir.clone()),
    })?;

    let wasm_path = plugins_dir.join("plugin.wasm");
    std::fs::rename(&tmp_wasm, &wasm_path)
        .or_else(|_| {
            // rename fails across filesystems; fall back to copy+delete
            std::fs::copy(&tmp_wasm, &wasm_path).map(|_| ())
        })
        .map_err(|e| SerializableError {
            kind: "IoError".to_string(),
            message: format!("Failed to write plugin WASM: {e}"),
            path: Some(wasm_path.clone()),
        })?;
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // Clear cached Diaryx so next execute() picks up the new plugin.
    let app_state = app.state::<AppState>();
    {
        let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
        *diaryx_guard = None;
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
            log::error!("[execute] Command execution failed: {:?}", e);
            e.to_serializable()
        })?
    } else {
        // Normal mode: use real filesystem
        // Try to use cached Diaryx instance for performance
        let app_state = app.state::<AppState>();

        // First, try to get cached diaryx (fast path)
        let cached_diaryx = {
            let diaryx_guard = acquire_lock(&app_state.diaryx)?;
            diaryx_guard.as_ref().map(Arc::clone)
        };

        let diaryx = if let Some(cached) = cached_diaryx {
            log::trace!("[execute] Using cached Diaryx instance");
            cached
        } else {
            // No cached instance - need to create one (slow path, only happens once)
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
                let adapters = register_extism_plugins(&mut d);
                if let Some(plugin_adapters) = app.try_state::<PluginAdapters>() {
                    if let Ok(mut guard) = plugin_adapters.adapters.lock() {
                        for adapter in adapters {
                            use diaryx_core::plugin::Plugin;
                            guard.insert(adapter.manifest().id.0.clone(), adapter);
                        }
                    }
                }
            }
            #[cfg(feature = "extism-plugins")]
            {
                if let Some(extism_sync_state) = app.try_state::<ExtismSyncState>() {
                    if let Ok(guard) = extism_sync_state.plugin.lock() {
                        if let Some(ref plugin) = *guard {
                            d.plugin_registry_mut()
                                .register_workspace_plugin(Arc::clone(plugin)
                                    as Arc<dyn diaryx_core::plugin::WorkspacePlugin>);
                            log::debug!("[execute] Registered Extism sync plugin");
                        }
                    }
                }
            }
            let new_diaryx = Arc::new(d);

            // Initialize plugins (seeds workspace root and link format)
            new_diaryx.init_plugins().await.ok();

            // Cache the new instance for future commands
            {
                let mut diaryx_guard = acquire_lock(&app_state.diaryx)?;
                *diaryx_guard = Some(Arc::clone(&new_diaryx));
                log::debug!("[execute] Cached Diaryx instance for future commands");
            }

            new_diaryx
        };

        diaryx.execute(cmd).await.map_err(|e| {
            log::error!("[execute] Command execution failed: {:?}", e);
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

        // Workspace goes in Documents so users can access via Files app
        let default_workspace = document_dir.join("Diaryx");
        // Config stays in Application Support (internal)
        let config_path = data_dir.join("config.toml");

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            is_mobile: true,
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
        let default_workspace = path_resolver
            .home_dir()
            .unwrap_or_else(|_| document_dir.clone())
            .join("diaryx");

        Ok(AppPaths {
            data_dir,
            document_dir,
            default_workspace,
            config_path,
            is_mobile: false,
        })
    }
}

/// Get the app paths for the current platform
#[tauri::command]
pub fn get_app_paths<R: Runtime>(app: AppHandle<R>) -> Result<AppPaths, SerializableError> {
    get_platform_paths(&app)
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

        let fs = SyncToAsyncFs::new(RealFileSystem);

        // Load existing config or create new one
        let mut config = if paths.config_path.exists() {
            Config::load_from(&fs, &paths.config_path)
                .await
                .unwrap_or_else(|_| Config::new(paths.default_workspace.clone()))
        } else {
            Config::new(paths.default_workspace.clone())
        };

        // Update workspace path
        config.default_workspace = selected_path.clone();

        // Save config
        config
            .save_to(&fs, &paths.config_path)
            .await
            .map_err(|e| e.to_serializable())?;

        // Initialize workspace if it doesn't exist
        let ws = Workspace::new(SyncToAsyncFs::new(RealFileSystem));
        let workspace_initialized = match ws.find_root_index_in_dir(&selected_path).await {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => false,
        };

        if !workspace_initialized {
            log::info!(
                "[pick_workspace_folder] Initializing workspace at {:?}",
                selected_path
            );
            ws.init_workspace(&selected_path, Some("My Workspace"), None)
                .await
                .map_err(|e| e.to_serializable())?;
        }

        Ok(Some(AppPaths {
            data_dir: paths.data_dir,
            document_dir: paths.document_dir,
            default_workspace: selected_path,
            config_path: paths.config_path,
            is_mobile: paths.is_mobile,
        }))
    }
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
    let config = if paths.is_mobile {
        // On mobile, use the platform-specific Documents/Diaryx path
        log::info!(
            "[initialize_app] Mobile: using platform workspace path: {:?}",
            paths.default_workspace
        );
        Config::new(paths.default_workspace.clone())
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

    // Use the workspace path from config (may differ from platform default)
    let actual_workspace = config.default_workspace.clone();
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
    }

    // Return paths with the actual workspace from config
    Ok(AppPaths {
        data_dir: paths.data_dir,
        document_dir: paths.document_dir,
        default_workspace: actual_workspace,
        config_path: paths.config_path,
        is_mobile: paths.is_mobile,
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

/// Export workspace to a specific format (DOCX, EPUB, PDF, etc.) using pandoc
///
/// For markdown and HTML, uses the built-in pipeline. For other formats,
/// shells out to the native `pandoc` binary.
#[tauri::command]
pub async fn export_to_format<R: Runtime>(
    app: AppHandle<R>,
    workspace_path: Option<String>,
    format: String,
    audience: Option<String>,
) -> Result<ExportResult, SerializableError> {
    use diaryx_core::export::Exporter;
    use diaryx_core::pandoc;
    use std::io::Write;
    use tauri_plugin_dialog::DialogExt;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    // Validate format
    if !pandoc::is_supported_format(&format) {
        return Err(SerializableError {
            kind: "ExportError".to_string(),
            message: format!(
                "Unsupported format: '{}'. Supported: {}",
                format,
                pandoc::SUPPORTED_FORMATS.join(", ")
            ),
            path: None,
        });
    }

    // Check pandoc availability for formats that need it
    if pandoc::requires_pandoc(&format) && !pandoc::is_pandoc_available() {
        return Err(SerializableError {
            kind: "PandocNotFound".to_string(),
            message: "pandoc is not installed. Install from https://pandoc.org/installing.html"
                .to_string(),
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
    let ext = pandoc::format_extension(&format);
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
            // Convert via pandoc (or comrak for html in future)
            let new_path = relative_str.replace(".md", &format!(".{}", ext));
            let converted = if pandoc::requires_pandoc(&format) || format == "html" {
                pandoc::convert_content(&content, &format, true)
            } else {
                Ok(content.into_bytes())
            };

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
                        "[ExportFormat] pandoc conversion failed for {}: {}",
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
) -> Result<AppPaths, SerializableError> {
    log::info!(
        "[reinitialize_workspace] Reinitializing for workspace: {}",
        workspace_path,
    );

    // 1. Clear cached diaryx (forces re-creation on next execute())
    let state = app.state::<AppState>();
    {
        *acquire_lock(&state.diaryx)? = None;
    }

    // 2. Ensure workspace directory exists
    let ws_path = PathBuf::from(&workspace_path);

    // On iOS, the sandbox container UUID changes between launches, so stored
    // absolute paths become invalid. Re-resolve by extracting the workspace
    // folder name and joining it to the current document_dir.
    #[cfg(target_os = "ios")]
    let ws_path = {
        let paths = get_platform_paths(&app)?;
        if ws_path.is_absolute() {
            if let Some(name) = ws_path.file_name() {
                paths.document_dir.join(name)
            } else {
                paths.default_workspace
            }
        } else {
            paths.document_dir.join(&ws_path)
        }
    };

    std::fs::create_dir_all(&ws_path).map_err(|e| SerializableError {
        kind: "IoError".to_string(),
        message: format!("Failed to create workspace directory: {}", e),
        path: Some(ws_path.clone()),
    })?;

    // 3. Update AppState
    {
        *acquire_lock(&state.workspace_path)? = Some(ws_path.clone());
    }

    // 4. Return AppPaths
    let paths = get_platform_paths(&app)?;
    Ok(AppPaths {
        data_dir: paths.data_dir,
        document_dir: paths.document_dir,
        default_workspace: ws_path,
        config_path: paths.config_path,
        is_mobile: paths.is_mobile,
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
