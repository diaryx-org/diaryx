//! Native filesystem implementation backed by `std::fs`.
//!
//! `RealFileSystem` implements [`diaryx_core::fs::FileSystem`]. Wrap it with
//! [`diaryx_core::fs::SyncToAsyncFs`] to use it with the async-first core
//! APIs (`Workspace`, `Validator`, `Exporter`, etc.).

use std::fs::{self, OpenOptions};
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::Path;

#[allow(deprecated)]
use diaryx_core::fs::FileSystem;
use diaryx_core::fs::{DirEntry, FileType, Metadata};

/// Synchronous [`FileSystem`] implementation backed by [`std::fs`].
///
/// Only available on native targets (not WebAssembly).
#[derive(Clone, Copy)]
pub struct RealFileSystem;

fn file_type_from_std(ft: std::fs::FileType) -> FileType {
    if ft.is_symlink() {
        FileType::symlink()
    } else if ft.is_dir() {
        FileType::dir()
    } else {
        FileType::file()
    }
}

fn metadata_from_std(m: std::fs::Metadata) -> Metadata {
    Metadata::new(
        file_type_from_std(m.file_type()),
        m.len(),
        m.modified().ok(),
    )
    .with_accessed(m.accessed().ok())
    .with_created(m.created().ok())
}

impl RealFileSystem {
    /// Compute the SHA-256 hash of a file (streaming) and return it as
    /// lowercase hex. Provided as an inherent method since `crossfs` does
    /// not include hashing in its trait surface.
    pub fn sha256_hex(path: &Path) -> Result<String> {
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
}

#[allow(deprecated)]
impl FileSystem for RealFileSystem {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        fs::read(path)
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path)
    }

    fn read_dir(&self, dir: &Path) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            entries.push(DirEntry::new(entry.path(), file_type_from_std(ft)));
        }
        Ok(entries)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        fs::create_dir(path)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        fs::remove_dir(path)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        fs::remove_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
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

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        fs::metadata(path).map(metadata_from_std)
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        fs::symlink_metadata(path).map(metadata_from_std)
    }

    fn create_new(&self, path: &Path, contents: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        // Atomic create-new: O_CREAT | O_EXCL
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(contents)
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<u64> {
        // std::fs::copy uses platform-native fast paths (clonefile / reflink /
        // CopyFileEx) when available, so we override the default whole-buffer
        // implementation here.
        if let Some(parent) = to.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)
    }

    fn canonicalize(&self, path: &Path) -> Result<std::path::PathBuf> {
        // Real symlink resolution; the default impl only handles `.` / `..`.
        fs::canonicalize(path)
    }
}
