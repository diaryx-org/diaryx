//! Host-bridged synchronous filesystem for the sync Extism guest.
//!
//! Implements `diaryx_core::fs::FileSystem` by delegating to host function calls.
//! Used to find the root index file via `find_root_index_in_dir_sync`.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::FileSystem;
use diaryx_plugin_sdk::host;

/// Synchronous filesystem backed by Extism host function calls.
pub struct HostFs;

impl FileSystem for HostFs {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        host::fs::read_file(&path.to_string_lossy()).map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        host::fs::write_file(&path.to_string_lossy(), content)
            .map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn create_new(&self, path: &Path, content: &str) -> Result<()> {
        let path_str = path.to_string_lossy();
        let exists =
            host::fs::file_exists(&path_str).map_err(|e| Error::new(ErrorKind::Other, e))?;
        if exists {
            return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
        }
        host::fs::write_file(&path_str, content).map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        host::fs::delete_file(&path.to_string_lossy()).map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let prefix = dir.to_string_lossy();
        let files = host::fs::list_dir(&prefix).map_err(|e| Error::new(ErrorKind::Other, e))?;
        Ok(files
            .into_iter()
            .filter(|f| f.ends_with(".md"))
            .map(PathBuf::from)
            .collect())
    }

    fn exists(&self, path: &Path) -> bool {
        host::fs::file_exists(&path.to_string_lossy()).unwrap_or(false)
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.extension().is_none()
    }

    fn move_file(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "move_file not supported in sync plugin",
        ))
    }
}
