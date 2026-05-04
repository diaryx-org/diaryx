//! File System Access API implementation of AsyncFileSystem.
//!
//! Uses the File System Access API to read/write files in a user-selected
//! directory on their actual filesystem. Unlike OPFS, this edits real files.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { FsaFileSystem, DiaryxBackend } from './wasm/diaryx_wasm.js';
//!
//! // User must trigger this via a gesture (click/keypress)
//! const dirHandle = await window.showDirectoryPicker();
//! const backend = await DiaryxBackend.createFromDirectoryHandle(dirHandle);
//! ```
//!
//! ## Browser Support
//! - Chrome/Edge: ✅ Supported
//! - Firefox: ❌ Not supported
//! - Safari: ❌ Not supported

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use diaryx_core::fs::crossfs;
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use futures::StreamExt;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use opfs::persistent::{self, DirectoryHandle};
use opfs::{
    CreateWritableOptions, DirectoryEntry, DirectoryHandle as DirectoryHandleTrait,
    FileHandle as FileHandleTrait, FileSystemRemoveOptions, GetDirectoryHandleOptions,
    GetFileHandleOptions, WritableFileStream as WritableFileStreamTrait,
};

// ============================================================================
// FsaFileSystem Implementation
// ============================================================================

/// AsyncFileSystem implementation backed by File System Access API.
#[wasm_bindgen]
pub struct FsaFileSystem {
    root: DirectoryHandle,
}

impl Clone for FsaFileSystem {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

#[wasm_bindgen]
impl FsaFileSystem {
    /// Create a new FsaFileSystem from a user-selected directory handle.
    #[wasm_bindgen(js_name = "fromHandle")]
    pub fn from_handle(handle: web_sys::FileSystemDirectoryHandle) -> Self {
        let root = DirectoryHandle::from(handle);
        Self { root }
    }
}

// ============================================================================
// Helper Functions (mirror those in opfs_fs.rs)
// ============================================================================

async fn get_or_create_parent_dir(
    root: &DirectoryHandle,
    path: &Path,
) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    if let Some(parent) = path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                let options = GetDirectoryHandleOptions { create: true };
                current = current
                    .get_directory_handle_with_options(&name_str, &options)
                    .await?;
            }
        }
    }

    Ok(current)
}

async fn get_parent_dir(
    root: &DirectoryHandle,
    path: &Path,
) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    if let Some(parent) = path.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                let options = GetDirectoryHandleOptions { create: false };
                current = current
                    .get_directory_handle_with_options(&name_str, &options)
                    .await?;
            }
        }
    }

    Ok(current)
}

async fn get_directory(root: &DirectoryHandle, path: &Path) -> persistent::Result<DirectoryHandle> {
    let mut current = root.clone();

    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();
            let options = GetDirectoryHandleOptions { create: false };
            current = current
                .get_directory_handle_with_options(&name_str, &options)
                .await?;
        }
    }

    Ok(current)
}

fn is_root(path: &Path) -> bool {
    path.as_os_str().is_empty() || path == Path::new(".")
}

fn get_filename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(String::from)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Invalid filename"))
}

fn opfs_to_io_error(e: persistent::Error) -> Error {
    let kind = dom_exception_name(&e)
        .map(|name| crossfs::error::dom_exception_kind(&name))
        .unwrap_or(ErrorKind::Other);
    Error::new(kind, format!("{:?}", e))
}

fn dom_exception_name(val: &JsValue) -> Option<String> {
    val.dyn_ref::<web_sys::DomException>().map(|e| e.name())
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

impl AsyncFileSystem for FsaFileSystem {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        Box::pin(async move {
            let dir = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let options = GetFileHandleOptions { create: false };
            let file = dir
                .get_file_handle_with_options(&filename, &options)
                .await
                .map_err(opfs_to_io_error)?;

            file.read().await.map_err(opfs_to_io_error)
        })
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let bytes = self.read(path).await?;
            String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))
        })
    }

    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        Box::pin(async move {
            let dir = if is_root(path) {
                self.root.clone()
            } else {
                get_directory(&self.root, path)
                    .await
                    .map_err(opfs_to_io_error)?
            };

            let mut entries_stream = dir.entries().await.map_err(opfs_to_io_error)?;

            let mut result = Vec::new();
            while let Some(entry_result) = entries_stream.next().await {
                if let Ok((name, entry)) = entry_result {
                    let full_path = if is_root(path) {
                        std::path::PathBuf::from(&name)
                    } else {
                        path.join(&name)
                    };
                    let file_type = match entry {
                        DirectoryEntry::File(_) => FileType::file(),
                        DirectoryEntry::Directory(_) => FileType::dir(),
                    };
                    result.push(DirEntry::new(full_path, file_type));
                }
            }
            Ok(result)
        })
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let contents = contents.to_vec();
        Box::pin(async move {
            let dir = get_or_create_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;

            let file_options = GetFileHandleOptions { create: true };
            let mut file = dir
                .get_file_handle_with_options(&filename, &file_options)
                .await
                .map_err(opfs_to_io_error)?;

            let write_options = CreateWritableOptions {
                keep_existing_data: false,
            };
            let mut writer = file
                .create_writable_with_options(&write_options)
                .await
                .map_err(opfs_to_io_error)?;

            writer
                .write_at_cursor_pos(contents)
                .await
                .map_err(opfs_to_io_error)?;
            writer.close().await.map_err(opfs_to_io_error)?;
            Ok(())
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if is_root(path) {
                return Ok(());
            }
            let parent = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let name = get_filename(path)?;
            let options = GetDirectoryHandleOptions { create: true };
            parent
                .get_directory_handle_with_options(&name, &options)
                .await
                .map_err(opfs_to_io_error)?;
            Ok(())
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut current = self.root.clone();

            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    let name_str = name.to_string_lossy();
                    let options = GetDirectoryHandleOptions { create: true };
                    current = current
                        .get_directory_handle_with_options(&name_str, &options)
                        .await
                        .map_err(opfs_to_io_error)?;
                }
            }
            Ok(())
        })
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut dir = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let filename = get_filename(path)?;
            dir.remove_entry(&filename)
                .await
                .map_err(opfs_to_io_error)?;
            Ok(())
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let entries = self.read_dir(path).await?;
            if !entries.is_empty() {
                return Err(Error::new(
                    ErrorKind::DirectoryNotEmpty,
                    format!("Directory not empty: {}", path.display()),
                ));
            }
            let mut parent = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let name = get_filename(path)?;
            parent.remove_entry(&name).await.map_err(opfs_to_io_error)?;
            Ok(())
        })
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut parent = get_parent_dir(&self.root, path)
                .await
                .map_err(opfs_to_io_error)?;
            let name = get_filename(path)?;
            let options = FileSystemRemoveOptions { recursive: true };
            parent
                .remove_entry_with_options(&name, &options)
                .await
                .map_err(opfs_to_io_error)?;
            Ok(())
        })
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let meta = self.metadata(from).await?;
            if !meta.is_file() {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "FSA rename only supports regular files",
                ));
            }
            if self.try_exists(to).await.unwrap_or(false) {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("Destination already exists: {}", to.display()),
                ));
            }
            let bytes = self.read(from).await?;
            self.write(to, &bytes).await?;
            self.remove_file(from).await?;
            Ok(())
        })
    }

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        Box::pin(async move {
            if is_root(path) {
                return Ok(Metadata::new(FileType::dir(), 0, None));
            }

            let parent = match get_parent_dir(&self.root, path).await {
                Ok(d) => d,
                Err(e) => return Err(opfs_to_io_error(e)),
            };
            let name = get_filename(path)?;

            let file_options = GetFileHandleOptions { create: false };
            if let Ok(file) = parent
                .get_file_handle_with_options(&name, &file_options)
                .await
            {
                let len = file.size().await.map(|s| s as u64).unwrap_or(0);
                return Ok(Metadata::new(FileType::file(), len, None));
            }

            let dir_options = GetDirectoryHandleOptions { create: false };
            if parent
                .get_directory_handle_with_options(&name, &dir_options)
                .await
                .is_ok()
            {
                return Ok(Metadata::new(FileType::dir(), 0, None));
            }

            Err(Error::new(
                ErrorKind::NotFound,
                format!("Path not found: {}", path.display()),
            ))
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let contents = contents.to_vec();
        Box::pin(async move {
            if self.try_exists(path).await.unwrap_or(false) {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("File already exists: {}", path.display()),
                ));
            }
            self.write(path, &contents).await
        })
    }
}
