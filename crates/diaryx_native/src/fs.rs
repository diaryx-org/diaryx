//! Native filesystem implementation backed by `std::fs`.
//!
//! `RealFileSystem` implements [`diaryx_core::fs::FileSystem`]. Wrap it with
//! [`diaryx_core::fs::SyncToAsyncFs`] to use it with the async-first core
//! APIs (`Workspace`, `Validator`, `Exporter`, etc.).

use std::fs::{self, OpenOptions};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::{Path, PathBuf};

use diaryx_core::fs::FileSystem;

/// Synchronous [`FileSystem`] implementation backed by [`std::fs`].
///
/// Only available on native targets (not WebAssembly).
#[derive(Clone, Copy)]
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    fn delete_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path)
    }

    fn create_new(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        // This atomic check prevents race conditions
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(content.as_bytes())
    }

    fn list_md_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "md") {
                    files.push(path);
                }
            }
        }
        Ok(files)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
    }

    fn move_file(&self, from: &Path, to: &Path) -> Result<()> {
        if !from.exists() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("Source file not found: {:?}", from),
            ));
        }
        if to.exists() {
            return Err(Error::new(
                ErrorKind::AlreadyExists,
                format!("Destination already exists: {:?}", to),
            ));
        }

        if let Some(parent) = to.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        fs::rename(from, to)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_symlink(&self, path: &Path) -> bool {
        path.is_symlink()
    }

    fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                files.push(entry.path());
            }
        }
        Ok(files)
    }

    fn read_binary(&self, path: &Path) -> Result<Vec<u8>> {
        fs::read(path)
    }

    fn hash_file(&self, path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};

        let mut file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];

        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }

        let hash = hasher.finalize();
        Ok(hash.iter().fold(String::with_capacity(64), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", b);
            s
        }))
    }

    fn write_binary(&self, path: &Path, content: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    }

    fn get_modified_time(&self, path: &Path) -> Option<i64> {
        fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64)
            })
    }

    fn get_file_size(&self, path: &Path) -> Option<u64> {
        fs::metadata(path).ok().map(|m| m.len())
    }
}
