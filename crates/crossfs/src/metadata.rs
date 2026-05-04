//! Metadata, file types, and directory entries.
//!
//! Mirrors the corresponding types in [`std::fs`].

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Metadata about a filesystem entry.
///
/// Returned by [`AsyncFileSystem::metadata`](crate::AsyncFileSystem::metadata)
/// and [`FileSystem::metadata`](crate::FileSystem::metadata). Mirrors
/// [`std::fs::Metadata`].
#[derive(Debug, Clone)]
pub struct Metadata {
    file_type: FileType,
    len: u64,
    modified: Option<SystemTime>,
}

impl Metadata {
    /// Construct a `Metadata` value. Backends use this to build the result of
    /// `metadata()`.
    pub fn new(file_type: FileType, len: u64, modified: Option<SystemTime>) -> Self {
        Self {
            file_type,
            len,
            modified,
        }
    }

    /// The file type (file / directory / symlink).
    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    /// True if this is a regular file.
    pub fn is_file(&self) -> bool {
        self.file_type.is_file()
    }

    /// True if this is a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// True if this is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// File size in bytes. Zero for directories and unsupported entries.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Convenience: `len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Last modification time.
    ///
    /// Returns `Err(io::ErrorKind::Unsupported)` on backends that do not track
    /// modification time (e.g. `InMemoryFs`, some browser stores).
    pub fn modified(&self) -> io::Result<SystemTime> {
        self.modified.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "modification time not available on this backend",
            )
        })
    }
}

/// The kind of a filesystem entry.
///
/// Mirrors [`std::fs::FileType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    File,
    Dir,
    Symlink,
}

impl FileType {
    /// A regular file.
    pub const fn file() -> Self {
        Self {
            kind: FileKind::File,
        }
    }

    /// A directory.
    pub const fn dir() -> Self {
        Self {
            kind: FileKind::Dir,
        }
    }

    /// A symbolic link.
    pub const fn symlink() -> Self {
        Self {
            kind: FileKind::Symlink,
        }
    }

    /// True if this is a regular file.
    pub fn is_file(&self) -> bool {
        matches!(self.kind, FileKind::File)
    }

    /// True if this is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, FileKind::Dir)
    }

    /// True if this is a symbolic link.
    pub fn is_symlink(&self) -> bool {
        matches!(self.kind, FileKind::Symlink)
    }
}

/// A single entry returned by [`AsyncFileSystem::read_dir`](crate::AsyncFileSystem::read_dir).
///
/// Mirrors [`std::fs::DirEntry`]. The path returned is the full path to the
/// entry (parent directory joined with the file name).
#[derive(Debug, Clone)]
pub struct DirEntry {
    path: PathBuf,
    file_type: FileType,
}

impl DirEntry {
    /// Construct a `DirEntry`. Backends use this to build the result of
    /// `read_dir()`.
    pub fn new(path: PathBuf, file_type: FileType) -> Self {
        Self { path, file_type }
    }

    /// The full path to this entry.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The final component of this entry's path.
    pub fn file_name(&self) -> OsString {
        self.path
            .file_name()
            .map(|s| s.to_os_string())
            .unwrap_or_default()
    }

    /// The file type of this entry. Returns a `Result` for parity with
    /// [`std::fs::DirEntry::file_type`], which may need to stat on some
    /// platforms; here it never fails.
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(self.file_type)
    }
}
