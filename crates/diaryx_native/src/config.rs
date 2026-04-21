//! Native [`Config`] helpers.
//!
//! `diaryx_core::config::Config` is platform-agnostic: it knows how to
//! serialize/deserialize itself and how to read/write via any
//! [`AsyncFileSystem`]. This module supplies the native-only pieces:
//!
//! - Resolution of the on-disk config location (`~/.config/diaryx/config.md`)
//!   via the [`dirs`] crate.
//! - A native [`default_config`] helper whose `default_workspace` points at
//!   `~/diaryx`.
//! - Blocking `_sync` wrappers around the async core APIs via
//!   [`futures_lite::future::block_on`].
//! - The [`NativeConfigExt`] extension trait, which restores the
//!   pre-split API surface (`Config::load()`, `Config::save()`,
//!   `Config::config_path()`, etc.) for callers that `use
//!   diaryx_native::NativeConfigExt;`.

use std::path::{Path, PathBuf};

use diaryx_core::config::Config;
use diaryx_core::error::{DiaryxError, Result};
use diaryx_core::fs::{FileSystem, SyncToAsyncFs};

use crate::fs::RealFileSystem;

/// Return the user-level config file path (`~/.config/diaryx/config.md` on
/// Unix, equivalent locations on other native OSes).
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("diaryx").join("config.md"))
}

/// Construct a [`Config`] with the native default workspace at `~/diaryx`
/// (falling back to `./diaryx` if the home directory can't be resolved).
pub fn default_config() -> Config {
    let default_base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("diaryx");
    Config::new(default_base)
}

/// Native extension methods on [`Config`].
///
/// Bring this trait into scope to call the classic
/// `Config::load()` / `Config::save()` / `Config::config_path()` /
/// `Config::init()` / `Config::load_from_sync()` /
/// `Config::save_to_sync()` / `Config::load_from_or_default_sync()` APIs.
pub trait NativeConfigExt: Sized {
    /// See [`config_path`].
    fn config_path() -> Option<PathBuf>;

    /// See [`default_config`].
    fn default_native() -> Self;

    /// Load config from the default native location, returning the native
    /// default if the file doesn't exist.
    fn load() -> Result<Self>;

    /// Save config to the default native location.
    fn save(&self) -> Result<()>;

    /// Initialize config with the given default workspace and save it to
    /// the default native location.
    fn init(default_workspace: PathBuf) -> Result<Self>;

    /// Sync wrapper for [`Config::load_from`](diaryx_core::config::Config::load_from).
    fn load_from_sync<FS: FileSystem>(fs: FS, path: &Path) -> Result<Self>;

    /// Sync wrapper for [`Config::save_to`](diaryx_core::config::Config::save_to).
    fn save_to_sync<FS: FileSystem>(&self, fs: FS, path: &Path) -> Result<()>;

    /// Sync wrapper for
    /// [`Config::load_from_or_default`](diaryx_core::config::Config::load_from_or_default).
    fn load_from_or_default_sync<FS: FileSystem>(
        fs: FS,
        path: &Path,
        default_workspace: PathBuf,
    ) -> Self;
}

impl NativeConfigExt for Config {
    fn config_path() -> Option<PathBuf> {
        config_path()
    }

    fn default_native() -> Self {
        default_config()
    }

    fn load() -> Result<Self> {
        let Some(path) = config_path() else {
            return Ok(default_config());
        };
        if !path.exists() {
            return Ok(default_config());
        }
        <Self as NativeConfigExt>::load_from_sync(RealFileSystem, &path)
    }

    fn save(&self) -> Result<()> {
        let path = config_path().ok_or(DiaryxError::NoConfigDir)?;
        <Self as NativeConfigExt>::save_to_sync(self, RealFileSystem, &path)
    }

    fn init(default_workspace: PathBuf) -> Result<Self> {
        let config = Config::new(default_workspace);
        <Self as NativeConfigExt>::save(&config)?;
        Ok(config)
    }

    fn load_from_sync<FS: FileSystem>(fs: FS, path: &Path) -> Result<Self> {
        futures_lite::future::block_on(Config::load_from(&SyncToAsyncFs::new(fs), path))
    }

    fn save_to_sync<FS: FileSystem>(&self, fs: FS, path: &Path) -> Result<()> {
        futures_lite::future::block_on(self.save_to(&SyncToAsyncFs::new(fs), path))
    }

    fn load_from_or_default_sync<FS: FileSystem>(
        fs: FS,
        path: &Path,
        default_workspace: PathBuf,
    ) -> Self {
        futures_lite::future::block_on(Config::load_from_or_default(
            &SyncToAsyncFs::new(fs),
            path,
            default_workspace,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_diaryx_workspace() {
        let cfg = default_config();
        assert!(
            cfg.default_workspace.to_string_lossy().ends_with("diaryx"),
            "expected default_workspace to end in 'diaryx', got {:?}",
            cfg.default_workspace
        );
    }

    #[test]
    fn config_path_ends_in_config_md() {
        let path = config_path().expect("config_dir should resolve on native");
        assert!(path.ends_with("diaryx/config.md"));
    }

    #[test]
    fn round_trip_sync_wrappers() {
        use diaryx_core::fs::InMemoryFileSystem;

        let fs = InMemoryFileSystem::new();
        let cfg = Config::new(PathBuf::from("/tmp/test-ws"));
        let path = Path::new("/cfg/config.md");

        <Config as NativeConfigExt>::save_to_sync(&cfg, fs.clone(), path).unwrap();
        let loaded = <Config as NativeConfigExt>::load_from_sync(fs, path).unwrap();
        assert_eq!(loaded.default_workspace, cfg.default_workspace);
    }
}
