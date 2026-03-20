//! Configuration types for Diaryx.
//!
//! This module provides the [`Config`] struct which stores user preferences
//! and workspace settings. Configuration is persisted as TOML (typically at
//! `~/.config/diaryx/config.toml` on Unix systems).
//!
//! # Key Configuration Fields
//!
//! - `default_workspace`: Primary workspace directory path
//! - `editor`: Preferred editor command
//! - `link_format`: Format for `part_of`/`contents`/`attachments` links
//! - `sync_*`: Cloud synchronization settings
//!
//! # Async-first Design
//!
//! Use `Config::load_from()` with an `AsyncFileSystem` to load config.
//! For synchronous contexts, use the `_sync` variants or wrap with
//! `SyncToAsyncFs` and use `block_on()`.
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::config::Config;
//! use std::path::PathBuf;
//!
//! // Create a new config
//! let config = Config::new(PathBuf::from("/home/user/diary"));
//!
//! // Load from default location (native only)
//! let config = Config::load()?;
//!
//! // Access config values
//! let workspace = config.default_workspace.clone();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "toml-config")]
use crate::error::{DiaryxError, Result};
#[cfg(feature = "toml-config")]
use crate::fs::AsyncFileSystem;
#[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
use crate::fs::{FileSystem, SyncToAsyncFs};
use crate::link_parser::LinkFormat;
use crate::workspace_registry::{WorkspaceEntry, WorkspaceRegistry};

/// `Config` is a data structure that represents the parts of Diaryx that the user can configure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default workspace directory
    /// This is the main directory for your workspace/journal
    #[serde(alias = "base_dir")]
    pub default_workspace: PathBuf,

    /// Preferred editor (falls back to $EDITOR if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,

    /// Format for `part_of`, `contents`, and `attachments` links in frontmatter.
    /// Used by CLI; web/tauri reads from WorkspaceConfig instead.
    #[serde(default, skip_serializing_if = "is_default_link_format")]
    pub link_format: LinkFormat,

    // ========================================================================
    // Sync configuration
    // ========================================================================
    /// Sync server URL (e.g., "https://app.diaryx.org/api")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_server_url: Option<String>,

    /// Session token for authenticated sync
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_session_token: Option<String>,

    /// Email address used for sync authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_email: Option<String>,

    /// Workspace ID for sync (identifies the remote workspace)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_workspace_id: Option<String>,

    // ========================================================================
    // Git version history configuration
    // ========================================================================
    /// Git-backed version history settings
    #[serde(default, skip_serializing_if = "GitConfig::is_default")]
    pub git: GitConfig,

    // ========================================================================
    // Multi-workspace registry
    // ========================================================================
    /// Registered workspaces. Each entry has a stable `local-<uuid>` ID.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceEntry>,

    /// Optional native bookmark data keyed by workspace path.
    ///
    /// macOS sandboxed builds use this to persist security-scoped bookmarks for
    /// workspace folders selected by the user. Other platforms ignore it.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub workspace_bookmarks: HashMap<String, String>,

    /// Whether iCloud Drive storage is enabled (iOS only).
    /// When true, the workspace is stored in the iCloud container directory.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub icloud_enabled: bool,
}

/// Configuration for git-backed version history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitConfig {
    /// Whether to automatically commit on workspace changes
    #[serde(default)]
    pub auto_commit: bool,

    /// Interval in minutes between auto-commits (default: 30)
    #[serde(default = "default_auto_commit_interval")]
    pub auto_commit_interval_minutes: u32,
}

fn default_auto_commit_interval() -> u32 {
    30
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            auto_commit: false,
            auto_commit_interval_minutes: default_auto_commit_interval(),
        }
    }
}

impl GitConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn is_default_link_format(format: &LinkFormat) -> bool {
    *format == LinkFormat::default()
}

impl Config {
    /// Alias for backwards compatibility
    pub fn base_dir(&self) -> &PathBuf {
        &self.default_workspace
    }

    /// Create a new config with the given workspace directory
    pub fn new(default_workspace: PathBuf) -> Self {
        Self {
            default_workspace,
            editor: None,
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
            git: GitConfig::default(),
            workspaces: Vec::new(),
            workspace_bookmarks: HashMap::new(),
            icloud_enabled: false,
        }
    }

    /// Create a config with workspace directory and optional editor/template values
    pub fn with_options(
        default_workspace: PathBuf,
        editor: Option<String>,
        _default_template: Option<String>,
    ) -> Self {
        Self {
            default_workspace,
            editor,
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
            git: GitConfig::default(),
            workspaces: Vec::new(),
            workspace_bookmarks: HashMap::new(),
            icloud_enabled: false,
        }
    }

    /// Return the stored bookmark for a workspace path, if present.
    pub fn workspace_bookmark(&self, path: &std::path::Path) -> Option<&str> {
        self.workspace_bookmarks
            .get(&path.to_string_lossy().into_owned())
            .map(String::as_str)
    }

    /// Store or replace the bookmark associated with a workspace path.
    pub fn set_workspace_bookmark(&mut self, path: PathBuf, bookmark: String) {
        self.workspace_bookmarks
            .insert(path.to_string_lossy().into_owned(), bookmark);
    }

    /// Build a [`WorkspaceRegistry`] from the config's workspace list.
    ///
    /// If `workspaces` is empty but `default_workspace` exists, a synthetic
    /// entry is included so callers always see at least one workspace.
    pub fn workspace_registry(&self) -> WorkspaceRegistry {
        let mut reg = WorkspaceRegistry {
            entries: self.workspaces.clone(),
            default_id: None,
        };

        // Find the entry whose path matches default_workspace and mark it as default
        if let Some(entry) = reg.find_by_path(&self.default_workspace) {
            reg.default_id = Some(entry.id.clone());
        }

        reg
    }

    /// Write registry changes back into the config fields.
    pub fn apply_registry(&mut self, registry: &WorkspaceRegistry) {
        self.workspaces = registry.entries.clone();
        // Update default_workspace path if the registry has a default with a path
        if let Some(entry) = registry.default_entry()
            && let Some(ref path) = entry.path
        {
            self.default_workspace = path.clone();
        }
    }

    // ========================================================================
    // AsyncFileSystem-based methods (work on all platforms including WASM)
    // ========================================================================

    /// Load config from a specific path using an AsyncFileSystem.
    #[cfg(feature = "toml-config")]
    pub async fn load_from<FS: AsyncFileSystem>(fs: &FS, path: &std::path::Path) -> Result<Self> {
        let contents = fs
            .read_to_string(path)
            .await
            .map_err(|e| DiaryxError::FileRead {
                path: path.to_path_buf(),
                source: e,
            })?;

        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to a specific path using an AsyncFileSystem.
    #[cfg(feature = "toml-config")]
    pub async fn save_to<FS: AsyncFileSystem>(
        &self,
        fs: &FS,
        path: &std::path::Path,
    ) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs.create_dir_all(parent).await?;
        }

        let contents = toml::to_string_pretty(self)?;
        fs.write_file(path, &contents).await?;
        Ok(())
    }

    /// Load config from an AsyncFileSystem, returning default if not found.
    #[cfg(feature = "toml-config")]
    pub async fn load_from_or_default<FS: AsyncFileSystem>(
        fs: &FS,
        path: &std::path::Path,
        default_workspace: PathBuf,
    ) -> Self {
        match Self::load_from(fs, path).await {
            Ok(config) => config,
            Err(_) => Self::new(default_workspace),
        }
    }

    // ========================================================================
    // Sync wrappers (compatibility layer). Prefer the async APIs above.
    // ========================================================================
    //
    // IMPORTANT:
    // These wrappers are only available on non-WASM targets because they require a
    // blocking executor. On WASM, filesystem access is expected to be async.

    /// Sync wrapper for [`Config::load_from`].
    #[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
    pub fn load_from_sync<FS: FileSystem>(fs: FS, path: &std::path::Path) -> Result<Self> {
        futures_lite::future::block_on(Self::load_from(&SyncToAsyncFs::new(fs), path))
    }

    /// Sync wrapper for [`Config::save_to`].
    #[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
    pub fn save_to_sync<FS: FileSystem>(&self, fs: FS, path: &std::path::Path) -> Result<()> {
        futures_lite::future::block_on(self.save_to(&SyncToAsyncFs::new(fs), path))
    }

    /// Sync wrapper for [`Config::load_from_or_default`].
    #[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
    pub fn load_from_or_default_sync<FS: FileSystem>(
        fs: FS,
        path: &std::path::Path,
        default_workspace: PathBuf,
    ) -> Self {
        futures_lite::future::block_on(Self::load_from_or_default(
            &SyncToAsyncFs::new(fs),
            path,
            default_workspace,
        ))
    }
}

// ============================================================================
// Native-only implementation (not available in WASM)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
impl Default for Config {
    fn default() -> Self {
        let default_base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("diaryx");

        Self {
            default_workspace: default_base,
            editor: None,
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
            git: GitConfig::default(),
            workspaces: Vec::new(),
            workspace_bookmarks: HashMap::new(),
            icloud_enabled: false,
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "toml-config"))]
impl Config {
    /// Get the config file path (~/.config/diaryx/config.toml)
    /// Only available on native platforms
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("diaryx").join("config.toml"))
    }

    /// Load config from default location, or return default if file doesn't exist
    /// Only available on native platforms
    pub fn load() -> Result<Self> {
        if let Some(path) = Self::config_path()
            && path.exists()
        {
            let contents = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&contents)?;
            return Ok(config);
        }

        // Return default config if file doesn't exist
        Ok(Config::default())
    }

    /// Save config to default location
    /// Only available on native platforms
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path().ok_or(DiaryxError::NoConfigDir)?;

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;

        Ok(())
    }

    /// Initialize config with user-provided values
    /// Only available on native platforms
    pub fn init(default_workspace: PathBuf) -> Result<Self> {
        Self::init_with_options(default_workspace)
    }

    /// Initialize config with user-provided values.
    /// Only available on native platforms
    pub fn init_with_options(default_workspace: PathBuf) -> Result<Self> {
        let config = Config {
            default_workspace,
            editor: None,
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
            git: GitConfig::default(),
            workspaces: Vec::new(),
            workspace_bookmarks: HashMap::new(),
            icloud_enabled: false,
        };

        config.save()?;
        Ok(config)
    }
}

// ============================================================================
// WASM-specific implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
impl Default for Config {
    fn default() -> Self {
        // In WASM, we use a simple default path
        // The actual workspace location will be virtual
        Self {
            default_workspace: PathBuf::from("/workspace"),
            editor: None,
            link_format: LinkFormat::default(),
            sync_server_url: None,
            sync_session_token: None,
            sync_email: None,
            sync_workspace_id: None,
            git: GitConfig::default(),
            workspaces: Vec::new(),
            workspace_bookmarks: HashMap::new(),
            icloud_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_registry_from_empty_config() {
        let config = Config::new(PathBuf::from("/home/user/journal"));
        let reg = config.workspace_registry();
        assert!(reg.entries.is_empty());
        assert!(reg.default_id.is_none());
    }

    #[test]
    fn workspace_registry_marks_default() {
        let mut config = Config::new(PathBuf::from("/home/user/journal"));
        config.workspaces.push(WorkspaceEntry {
            id: "local-abc".into(),
            name: "journal".into(),
            path: Some(PathBuf::from("/home/user/journal")),
        });
        let reg = config.workspace_registry();
        assert_eq!(reg.default_id.as_deref(), Some("local-abc"));
    }

    #[test]
    fn apply_registry_updates_default_workspace() {
        let mut config = Config::new(PathBuf::from("/old"));
        let mut reg = WorkspaceRegistry::default();
        let id = reg
            .register("new-ws".into(), Some(PathBuf::from("/new")))
            .id
            .clone();
        reg.set_default(&id);
        config.apply_registry(&reg);
        assert_eq!(config.default_workspace, PathBuf::from("/new"));
        assert_eq!(config.workspaces.len(), 1);
    }

    #[test]
    fn toml_round_trip_with_workspaces() {
        let mut config = Config::new(PathBuf::from("/ws"));
        config.workspaces.push(WorkspaceEntry {
            id: "local-123".into(),
            name: "personal".into(),
            path: Some(PathBuf::from("/ws")),
        });
        config.set_workspace_bookmark(PathBuf::from("/ws"), "bookmark-data".into());
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.workspaces.len(), 1);
        assert_eq!(parsed.workspaces[0].id, "local-123");
        assert_eq!(parsed.workspaces[0].name, "personal");
        assert_eq!(
            parsed.workspace_bookmark(PathBuf::from("/ws").as_path()),
            Some("bookmark-data")
        );
    }

    #[test]
    fn toml_round_trip_without_workspaces() {
        let config = Config::new(PathBuf::from("/ws"));
        let toml_str = toml::to_string_pretty(&config).unwrap();
        // workspaces should be omitted when empty
        assert!(!toml_str.contains("workspaces"));
        assert!(!toml_str.contains("workspace_bookmarks"));
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert!(parsed.workspaces.is_empty());
        assert!(parsed.workspace_bookmarks.is_empty());
    }
}
