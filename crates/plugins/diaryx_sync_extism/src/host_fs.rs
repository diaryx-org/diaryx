//! Host-bridged synchronous filesystem for the sync Extism guest.
//!
//! Implements `diaryx_core::fs::FileSystem` by delegating to host function calls.
//! Used to find the root index file via `find_root_index_in_dir_sync`.

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

#[allow(deprecated)]
use diaryx_core::fs::FileSystem;
use diaryx_core::fs::{DirEntry, FileType, Metadata};
use diaryx_plugin_sdk::host;

/// Synchronous filesystem backed by Extism host function calls.
pub struct HostFs;

#[allow(deprecated)]
impl FileSystem for HostFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        self.read_to_string(path).map(|s| s.into_bytes())
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        host::fs::read_file(&path.to_string_lossy()).map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn read_dir(&self, dir: &Path) -> Result<Vec<DirEntry>> {
        let files = host::fs::list_dir(&dir.to_string_lossy())
            .map_err(|e| Error::new(ErrorKind::Other, e))?;
        Ok(files
            .into_iter()
            .map(|f| {
                let p = std::path::PathBuf::from(f);
                let ft = if p.extension().is_none() {
                    FileType::dir()
                } else {
                    FileType::file()
                };
                DirEntry::new(p, ft)
            })
            .collect())
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let s = std::str::from_utf8(contents)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "Sync plugin host_fs is text-only"))?;
        host::fs::write_file(&path.to_string_lossy(), s)
            .map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn create_dir(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        host::fs::delete_file(&path.to_string_lossy()).map_err(|e| Error::new(ErrorKind::Other, e))
    }

    fn remove_dir(&self, _path: &Path) -> Result<()> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "remove_dir not supported in sync plugin",
        ))
    }

    fn remove_dir_all(&self, _path: &Path) -> Result<()> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "remove_dir_all not supported in sync plugin",
        ))
    }

    fn rename(&self, _from: &Path, _to: &Path) -> Result<()> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "rename not supported in sync plugin",
        ))
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let exists = host::fs::file_exists(&path.to_string_lossy())
            .map_err(|e| Error::new(ErrorKind::Other, e))?;
        if !exists {
            return Err(Error::new(ErrorKind::NotFound, "Not found"));
        }
        // Sync plugin host has no `is_dir` primitive — extension heuristic.
        let ft = if path.extension().is_none() {
            FileType::dir()
        } else {
            FileType::file()
        };
        Ok(Metadata::new(ft, 0, None))
    }

    fn create_new(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let path_str = path.to_string_lossy();
        let exists =
            host::fs::file_exists(&path_str).map_err(|e| Error::new(ErrorKind::Other, e))?;
        if exists {
            return Err(Error::new(ErrorKind::AlreadyExists, "File already exists"));
        }
        let s = std::str::from_utf8(contents)
            .map_err(|_| Error::new(ErrorKind::InvalidData, "Sync plugin host_fs is text-only"))?;
        host::fs::write_file(&path_str, s).map_err(|e| Error::new(ErrorKind::Other, e))
    }
}
