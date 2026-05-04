//! In-memory filesystem.
//!
//! Available on every target. Useful for tests, sandboxes, and clients (e.g.
//! WASM frontends) that load a workspace into memory and persist it
//! out-of-band.

use std::collections::{HashMap, HashSet};
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::metadata::{DirEntry, FileType, Metadata};
#[allow(deprecated)]
use crate::traits::FileSystem;

/// An in-memory filesystem.
///
/// Stores text and binary files separately and tracks directories
/// explicitly. Symlinks may be added with [`add_symlink`](Self::add_symlink);
/// reading through a symlink resolves to the target's content.
#[derive(Clone, Default)]
pub struct InMemoryFs {
    /// Text files stored as path -> content.
    files: Arc<RwLock<HashMap<PathBuf, String>>>,
    /// Binary files stored as path -> bytes.
    binary_files: Arc<RwLock<HashMap<PathBuf, Vec<u8>>>>,
    /// Directories that exist (implicitly created when files are added).
    directories: Arc<RwLock<HashSet<PathBuf>>>,
    /// Symlinks: source -> target. Reading the source returns the target's
    /// content; `symlink_metadata` reports the source as a symlink.
    symlinks: Arc<RwLock<HashMap<PathBuf, PathBuf>>>,
}

impl InMemoryFs {
    /// Create a new empty in-memory filesystem.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filesystem pre-populated with text files.
    pub fn with_files(entries: Vec<(PathBuf, String)>) -> Self {
        let fs = Self::new();
        {
            let mut files = fs.files.write().unwrap();
            let mut dirs = fs.directories.write().unwrap();

            for (path, content) in entries {
                let mut current = path.as_path();
                while let Some(parent) = current.parent() {
                    if !parent.as_os_str().is_empty() {
                        dirs.insert(parent.to_path_buf());
                    }
                    current = parent;
                }
                files.insert(path, content);
            }
        }
        fs
    }

    /// Load files from `(path_string, content)` tuples. Convenience for JS
    /// interop.
    pub fn load_from_entries(entries: Vec<(String, String)>) -> Self {
        let entries: Vec<(PathBuf, String)> = entries
            .into_iter()
            .map(|(path, content)| (PathBuf::from(path), content))
            .collect();
        Self::with_files(entries)
    }

    /// Export all text files as `(path_string, content)` tuples.
    pub fn export_entries(&self) -> Vec<(String, String)> {
        let files = self.files.read().unwrap();
        files
            .iter()
            .map(|(path, content)| (path.to_string_lossy().to_string(), content.clone()))
            .collect()
    }

    /// Export all binary files as `(path_string, content_bytes)` tuples.
    pub fn export_binary_entries(&self) -> Vec<(String, Vec<u8>)> {
        let binary_files = self.binary_files.read().unwrap();
        binary_files
            .iter()
            .map(|(path, content)| (path.to_string_lossy().to_string(), content.clone()))
            .collect()
    }

    /// Load binary files from `(path_string, content_bytes)` tuples.
    pub fn load_binary_entries(&self, entries: Vec<(String, Vec<u8>)>) {
        let mut binary_files = self.binary_files.write().unwrap();
        let mut dirs = self.directories.write().unwrap();

        for (path_str, content) in entries {
            let path = PathBuf::from(&path_str);
            let mut current = path.as_path();
            while let Some(parent) = current.parent() {
                if !parent.as_os_str().is_empty() {
                    dirs.insert(parent.to_path_buf());
                }
                current = parent;
            }
            binary_files.insert(path, content);
        }
    }

    /// All file paths currently stored.
    pub fn list_all_files(&self) -> Vec<PathBuf> {
        let files = self.files.read().unwrap();
        files.keys().cloned().collect()
    }

    /// Remove every file and directory from the filesystem.
    pub fn clear(&self) {
        let mut files = self.files.write().unwrap();
        let mut binary = self.binary_files.write().unwrap();
        let mut dirs = self.directories.write().unwrap();
        let mut links = self.symlinks.write().unwrap();
        files.clear();
        binary.clear();
        dirs.clear();
        links.clear();
    }

    /// Add a symlink from `link` to `target`. Reading `link` resolves the
    /// content of `target`; `symlink_metadata(link)` reports it as a symlink.
    pub fn add_symlink(&self, link: &Path, target: &Path) {
        let link = Self::normalize_path(link);
        let target = Self::normalize_path(target);

        if let Some(parent) = link.parent() {
            let mut dirs = self.directories.write().unwrap();
            let mut current = parent;
            loop {
                if current.as_os_str().is_empty() {
                    break;
                }
                dirs.insert(current.to_path_buf());
                match current.parent() {
                    Some(p) => current = p,
                    None => break,
                }
            }
        }

        let mut symlinks = self.symlinks.write().unwrap();
        symlinks.insert(link, target);
    }

    /// Helper to normalize paths (strip `.` and `..`).
    fn normalize_path(path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            use std::path::Component;
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !components.is_empty() {
                        components.pop();
                    }
                }
                c => components.push(c),
            }
        }
        components.iter().collect()
    }
}

#[allow(deprecated)]
impl FileSystem for InMemoryFs {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let normalized = Self::normalize_path(path);

        // Resolve symlinks for read.
        let resolved = {
            let symlinks = self.symlinks.read().unwrap();
            symlinks.get(&normalized).cloned().unwrap_or(normalized)
        };

        {
            let binary_files = self.binary_files.read().unwrap();
            if let Some(data) = binary_files.get(&resolved) {
                return Ok(data.clone());
            }
        }

        let files = self.files.read().unwrap();
        files
            .get(&resolved)
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("File not found: {:?}", path)))
    }

    fn read_to_string(&self, path: &Path) -> Result<String> {
        let normalized = Self::normalize_path(path);
        let resolved = {
            let symlinks = self.symlinks.read().unwrap();
            symlinks.get(&normalized).cloned().unwrap_or(normalized)
        };

        let files = self.files.read().unwrap();
        files
            .get(&resolved)
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("File not found: {:?}", path)))
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let normalized = Self::normalize_path(path);
        let files = self.files.read().unwrap();
        let binary_files = self.binary_files.read().unwrap();
        let dirs = self.directories.read().unwrap();
        let symlinks = self.symlinks.read().unwrap();

        let mut result = Vec::new();

        for entry in files.keys() {
            if entry.parent() == Some(&normalized) {
                result.push(DirEntry::new(entry.clone(), FileType::file()));
            }
        }
        for entry in binary_files.keys() {
            if entry.parent() == Some(&normalized) {
                result.push(DirEntry::new(entry.clone(), FileType::file()));
            }
        }
        for entry in dirs.iter() {
            if entry.parent() == Some(&normalized) && entry != &normalized {
                result.push(DirEntry::new(entry.clone(), FileType::dir()));
            }
        }
        for entry in symlinks.keys() {
            if entry.parent() == Some(&normalized) {
                result.push(DirEntry::new(entry.clone(), FileType::symlink()));
            }
        }

        Ok(result)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let normalized = Self::normalize_path(path);

        if let Some(parent) = normalized.parent() {
            self.create_dir_all(parent)?;
        }

        // If the bytes are valid UTF-8, store as text so that read_to_string
        // works (matches previous Diaryx behavior, where write_file/read_to_string
        // round-tripped via the text store).
        match std::str::from_utf8(contents) {
            Ok(s) => {
                let mut files = self.files.write().unwrap();
                files.insert(normalized.clone(), s.to_string());
                // Drop any prior binary entry at the same path.
                self.binary_files.write().unwrap().remove(&normalized);
            }
            Err(_) => {
                let mut binary = self.binary_files.write().unwrap();
                binary.insert(normalized.clone(), contents.to_vec());
                self.files.write().unwrap().remove(&normalized);
            }
        }

        Ok(())
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);
        if normalized.as_os_str().is_empty() {
            return Ok(());
        }
        let mut dirs = self.directories.write().unwrap();
        dirs.insert(normalized);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);
        let mut dirs = self.directories.write().unwrap();

        let mut current = normalized.as_path();
        loop {
            if !current.as_os_str().is_empty() {
                dirs.insert(current.to_path_buf());
            }
            match current.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => current = parent,
                _ => break,
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);

        {
            let mut files = self.files.write().unwrap();
            if files.remove(&normalized).is_some() {
                return Ok(());
            }
        }

        {
            let mut binary = self.binary_files.write().unwrap();
            if binary.remove(&normalized).is_some() {
                return Ok(());
            }
        }

        {
            let mut links = self.symlinks.write().unwrap();
            if links.remove(&normalized).is_some() {
                return Ok(());
            }
        }

        Err(Error::new(
            ErrorKind::NotFound,
            format!("File not found: {:?}", path),
        ))
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);
        // Verify directory is empty.
        let read_dir = self.read_dir(&normalized)?;
        if !read_dir.is_empty() {
            return Err(Error::new(
                ErrorKind::DirectoryNotEmpty,
                format!("Directory not empty: {:?}", path),
            ));
        }
        let mut dirs = self.directories.write().unwrap();
        if dirs.remove(&normalized) {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::NotFound,
                format!("Directory not found: {:?}", path),
            ))
        }
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        let normalized = Self::normalize_path(path);

        let mut files = self.files.write().unwrap();
        files.retain(|p, _| !p.starts_with(&normalized));
        let mut binary = self.binary_files.write().unwrap();
        binary.retain(|p, _| !p.starts_with(&normalized));
        let mut links = self.symlinks.write().unwrap();
        links.retain(|p, _| !p.starts_with(&normalized));
        let mut dirs = self.directories.write().unwrap();
        dirs.retain(|p| !p.starts_with(&normalized) && p != &normalized);

        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let from_norm = Self::normalize_path(from);
        let to_norm = Self::normalize_path(to);

        if from_norm == to_norm {
            return Ok(());
        }

        let is_dir = {
            let dirs = self.directories.read().unwrap();
            dirs.contains(&from_norm)
        };

        if is_dir {
            // Move every file rooted at from_norm.
            let files_to_move: Vec<(PathBuf, String)> = {
                let files = self.files.read().unwrap();
                files
                    .iter()
                    .filter(|(p, _)| p.starts_with(&from_norm))
                    .map(|(p, c)| (p.clone(), c.clone()))
                    .collect()
            };
            let binaries_to_move: Vec<(PathBuf, Vec<u8>)> = {
                let bin = self.binary_files.read().unwrap();
                bin.iter()
                    .filter(|(p, _)| p.starts_with(&from_norm))
                    .map(|(p, c)| (p.clone(), c.clone()))
                    .collect()
            };

            // Reject if destination already exists as a file or directory.
            {
                let files = self.files.read().unwrap();
                let bin = self.binary_files.read().unwrap();
                let dirs = self.directories.read().unwrap();
                if files.contains_key(&to_norm)
                    || bin.contains_key(&to_norm)
                    || dirs.contains(&to_norm)
                {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("Destination already exists: {:?}", to),
                    ));
                }
            }

            {
                let mut files = self.files.write().unwrap();
                for (old_path, content) in files_to_move {
                    files.remove(&old_path);
                    let relative = old_path.strip_prefix(&from_norm).unwrap();
                    files.insert(to_norm.join(relative), content);
                }
            }
            {
                let mut binary = self.binary_files.write().unwrap();
                for (old_path, content) in binaries_to_move {
                    binary.remove(&old_path);
                    let relative = old_path.strip_prefix(&from_norm).unwrap();
                    binary.insert(to_norm.join(relative), content);
                }
            }
            {
                let mut dirs = self.directories.write().unwrap();
                let old_dirs: Vec<PathBuf> = dirs
                    .iter()
                    .filter(|d| d.starts_with(&from_norm) || **d == from_norm)
                    .cloned()
                    .collect();
                for old_dir in old_dirs {
                    dirs.remove(&old_dir);
                    if old_dir == from_norm {
                        dirs.insert(to_norm.clone());
                    } else if let Ok(relative) = old_dir.strip_prefix(&from_norm) {
                        dirs.insert(to_norm.join(relative));
                    }
                }

                let mut current = to_norm.as_path();
                loop {
                    match current.parent() {
                        Some(parent) if !parent.as_os_str().is_empty() => {
                            dirs.insert(parent.to_path_buf());
                            current = parent;
                        }
                        _ => break,
                    }
                }
            }

            Ok(())
        } else {
            // Move a single file.
            {
                let files = self.files.read().unwrap();
                let bin = self.binary_files.read().unwrap();
                if !files.contains_key(&from_norm) && !bin.contains_key(&from_norm) {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("Source file not found: {:?}", from),
                    ));
                }

                if files.contains_key(&to_norm) || bin.contains_key(&to_norm) {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("Destination already exists: {:?}", to),
                    ));
                }
            }

            if let Some(parent) = to_norm.parent() {
                self.create_dir_all(parent)?;
            }

            {
                let mut files = self.files.write().unwrap();
                if let Some(content) = files.remove(&from_norm) {
                    files.insert(to_norm, content);
                    return Ok(());
                }
            }
            {
                let mut binary = self.binary_files.write().unwrap();
                if let Some(content) = binary.remove(&from_norm) {
                    binary.insert(to_norm, content);
                    return Ok(());
                }
            }

            Err(Error::new(
                ErrorKind::NotFound,
                format!("Source file not found: {:?}", from),
            ))
        }
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let normalized = Self::normalize_path(path);

        // Follow symlinks (matches std::fs::metadata).
        let resolved = {
            let symlinks = self.symlinks.read().unwrap();
            symlinks.get(&normalized).cloned().unwrap_or(normalized)
        };

        if let Some(data) = self.binary_files.read().unwrap().get(&resolved) {
            return Ok(Metadata::new(FileType::file(), data.len() as u64, None));
        }
        if let Some(content) = self.files.read().unwrap().get(&resolved) {
            return Ok(Metadata::new(FileType::file(), content.len() as u64, None));
        }
        if self.directories.read().unwrap().contains(&resolved) {
            return Ok(Metadata::new(FileType::dir(), 0, None));
        }

        Err(Error::new(
            ErrorKind::NotFound,
            format!("Path not found: {:?}", path),
        ))
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        let normalized = Self::normalize_path(path);

        // Don't follow symlinks: report the link itself.
        if self.symlinks.read().unwrap().contains_key(&normalized) {
            return Ok(Metadata::new(FileType::symlink(), 0, None));
        }
        // Otherwise the same as metadata.
        self.metadata(path)
    }

    fn create_new(&self, path: &Path, contents: &[u8]) -> Result<()> {
        let normalized = Self::normalize_path(path);

        {
            let files = self.files.read().unwrap();
            let bin = self.binary_files.read().unwrap();
            if files.contains_key(&normalized) || bin.contains_key(&normalized) {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("File already exists: {:?}", path),
                ));
            }
        }

        self.write(path, contents)
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn read_write_roundtrip() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("test.md"), b"Hello, World!").unwrap();
        assert_eq!(
            fs.read_to_string(Path::new("test.md")).unwrap(),
            "Hello, World!"
        );
        assert!(fs.try_exists(Path::new("test.md")).unwrap());
        fs.remove_file(Path::new("test.md")).unwrap();
        assert!(!fs.try_exists(Path::new("test.md")).unwrap());
    }

    #[test]
    fn create_new_rejects_existing() {
        let fs = InMemoryFs::new();
        fs.create_new(Path::new("new.md"), b"Content").unwrap();
        assert_eq!(fs.read_to_string(Path::new("new.md")).unwrap(), "Content");
        assert!(fs.create_new(Path::new("new.md"), b"Other").is_err());
    }

    #[test]
    fn create_dir_all_creates_parents_implicitly_via_write() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("a/b/c/file.md"), b"Content").unwrap();
        assert!(fs.metadata(Path::new("a")).unwrap().is_dir());
        assert!(fs.metadata(Path::new("a/b")).unwrap().is_dir());
        assert!(fs.metadata(Path::new("a/b/c")).unwrap().is_dir());
        assert!(fs.try_exists(Path::new("a/b/c/file.md")).unwrap());
    }

    #[test]
    fn read_dir_returns_immediate_children() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("dir/file1.md"), b"1").unwrap();
        fs.write(Path::new("dir/file2.md"), b"2").unwrap();
        fs.write(Path::new("dir/subdir/file3.md"), b"3").unwrap();

        let entries = fs.read_dir(Path::new("dir")).unwrap();
        let paths: Vec<PathBuf> = entries.iter().map(|e| e.path().to_path_buf()).collect();
        assert!(paths.contains(&PathBuf::from("dir/file1.md")));
        assert!(paths.contains(&PathBuf::from("dir/file2.md")));
        assert!(paths.contains(&PathBuf::from("dir/subdir")));
        assert!(!paths.contains(&PathBuf::from("dir/subdir/file3.md")));
    }

    #[test]
    fn export_then_import_roundtrip() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("file1.md"), b"Content 1").unwrap();
        fs.write(Path::new("dir/file2.md"), b"Content 2").unwrap();

        let entries = fs.export_entries();
        let fs2 = InMemoryFs::load_from_entries(entries);

        assert_eq!(
            fs2.read_to_string(Path::new("file1.md")).unwrap(),
            "Content 1"
        );
        assert_eq!(
            fs2.read_to_string(Path::new("dir/file2.md")).unwrap(),
            "Content 2"
        );
    }

    #[test]
    fn path_normalization() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("dir/file.md"), b"Content").unwrap();
        assert!(fs.try_exists(Path::new("dir/file.md")).unwrap());
        assert!(fs.try_exists(Path::new("dir/./file.md")).unwrap());
        assert!(fs.try_exists(Path::new("dir/subdir/../file.md")).unwrap());
    }

    #[test]
    fn symlink_metadata_distinguishes_links() {
        let fs = InMemoryFs::new();
        fs.write(Path::new("real.md"), b"hello").unwrap();
        fs.add_symlink(Path::new("link.md"), Path::new("real.md"));

        // metadata() follows the symlink.
        let m = fs.metadata(Path::new("link.md")).unwrap();
        assert!(m.is_file());
        assert!(!m.is_symlink());

        // symlink_metadata() does not.
        let lm = fs.symlink_metadata(Path::new("link.md")).unwrap();
        assert!(lm.is_symlink());

        // Reading through the link works.
        assert_eq!(fs.read_to_string(Path::new("link.md")).unwrap(), "hello");
    }
}
