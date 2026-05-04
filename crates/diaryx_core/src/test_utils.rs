//! Test utilities for diaryx_core.
//!
//! `MockFileSystem` is a tiny in-process filesystem used by unit tests in
//! this crate. Production code should use `InMemoryFileSystem`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[allow(deprecated)]
use crate::fs::FileSystem;
use crate::fs::{DirEntry, FileType, Metadata};

/// A mock filesystem for testing.
///
/// Uses `Arc<Mutex<HashMap>>` for thread-safety and allows cloning while
/// sharing the same underlying file storage. Directories are represented
/// as entries whose content is the sentinel `"<DIR>"`.
#[derive(Clone, Default)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, String>>>,
}

impl MockFileSystem {
    /// Create an empty mock filesystem.
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a file to the mock filesystem (builder pattern).
    pub fn with_file(self, path: &str, content: &str) -> Self {
        self.files
            .lock()
            .unwrap()
            .insert(PathBuf::from(path), content.to_string());
        self
    }

    /// Get the content of a file (for test assertions).
    pub fn get_content(&self, path: &str) -> Option<String> {
        self.files
            .lock()
            .unwrap()
            .get(&PathBuf::from(path))
            .cloned()
    }

    fn is_dir_sentinel(content: &str) -> bool {
        content == "<DIR>"
    }
}

#[allow(deprecated)]
impl FileSystem for MockFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read_to_string(path).map(|s| s.into_bytes())
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let files = self.files.lock().unwrap();
        match files.get(path) {
            Some(content) if Self::is_dir_sentinel(content) => {
                Err(io::Error::other("Is a directory"))
            }
            Some(content) => Ok(content.clone()),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "File not found")),
        }
    }

    fn read_dir(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        let files = self.files.lock().unwrap();
        let mut result = Vec::new();
        for (path, content) in files.iter() {
            if path.parent() == Some(dir) {
                let ft = if Self::is_dir_sentinel(content) {
                    FileType::dir()
                } else {
                    FileType::file()
                };
                result.push(DirEntry::new(path.clone(), ft));
            }
        }
        Ok(result)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let s = std::str::from_utf8(contents).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "MockFileSystem only stores UTF-8",
            )
        })?;
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), s.to_string());
        Ok(())
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        self.create_dir_all(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files.insert(path.to_path_buf(), "<DIR>".to_string());
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files.retain(|p, _| !p.starts_with(path) && p != path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if !files.contains_key(from) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
        }
        if files.contains_key(to) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists",
            ));
        }
        let content = files
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))?;
        files.insert(to.to_path_buf(), content);
        Ok(())
    }

    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        let files = self.files.lock().unwrap();
        match files.get(path) {
            Some(content) if Self::is_dir_sentinel(content) => {
                Ok(Metadata::new(FileType::dir(), 0, None))
            }
            Some(content) => Ok(Metadata::new(FileType::file(), content.len() as u64, None)),
            None => Err(io::Error::new(io::ErrorKind::NotFound, "Not found")),
        }
    }

    fn create_new(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if files.contains_key(path) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File exists"));
        }
        let s = std::str::from_utf8(contents).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "MockFileSystem only stores UTF-8",
            )
        })?;
        files.insert(path.to_path_buf(), s.to_string());
        Ok(())
    }
}
