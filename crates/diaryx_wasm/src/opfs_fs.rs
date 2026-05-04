//! OPFS (Origin Private File System) implementation of AsyncFileSystem.
//!
//! Uses the `opfs` crate to provide persistent file storage in browsers.
//! This backend works in both Web Workers (with sync access handles) and
//! the main thread (with async access).
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { OpfsFileSystem, DiaryxAsyncWorkspace } from './wasm/diaryx_wasm.js';
//!
//! const fs = await OpfsFileSystem.create();
//! const workspace = new DiaryxAsyncWorkspace(fs);
//! const tree = await workspace.getTree('workspace');
//! ```

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

use diaryx_core::fs::crossfs;
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use futures::StreamExt;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

use opfs::persistent::{self, DirectoryHandle};
use opfs::{
    CreateWritableOptions, DirectoryEntry, DirectoryHandle as DirectoryHandleTrait,
    FileHandle as FileHandleTrait, FileSystemRemoveOptions, GetDirectoryHandleOptions,
    GetFileHandleOptions, WritableFileStream as WritableFileStreamTrait,
};

// ============================================================================
// OpfsFileSystem Implementation
// ============================================================================

/// AsyncFileSystem implementation backed by OPFS (Origin Private File System).
#[wasm_bindgen]
pub struct OpfsFileSystem {
    root: DirectoryHandle,
}

impl Clone for OpfsFileSystem {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
        }
    }
}

/// Get OPFS root directory in a worker-compatible way.
async fn get_opfs_root() -> std::result::Result<web_sys::FileSystemDirectoryHandle, JsValue> {
    let global = js_sys::global();

    let navigator = js_sys::Reflect::get(&global, &JsValue::from_str("navigator"))?;
    if navigator.is_undefined() {
        return Err(JsValue::from_str(
            "No navigator object found in global scope",
        ));
    }

    let storage = js_sys::Reflect::get(&navigator, &JsValue::from_str("storage"))?;
    if storage.is_undefined() {
        return Err(JsValue::from_str("No storage object found on navigator"));
    }

    let get_directory = js_sys::Reflect::get(&storage, &JsValue::from_str("getDirectory"))?;
    let get_directory_fn = get_directory
        .dyn_ref::<js_sys::Function>()
        .ok_or_else(|| JsValue::from_str("getDirectory is not a function"))?;

    let promise = get_directory_fn.call0(&storage)?;
    let promise = promise.dyn_into::<js_sys::Promise>()?;

    let result = JsFuture::from(promise).await?;
    result.dyn_into::<web_sys::FileSystemDirectoryHandle>()
}

#[wasm_bindgen]
impl OpfsFileSystem {
    /// Create a new OpfsFileSystem with the default app directory.
    #[wasm_bindgen]
    pub async fn create() -> std::result::Result<OpfsFileSystem, JsValue> {
        Self::create_with_name("My Journal").await
    }

    /// Create a new OpfsFileSystem with a custom root directory name.
    #[wasm_bindgen(js_name = "createWithName")]
    pub async fn create_with_name(root_name: &str) -> std::result::Result<OpfsFileSystem, JsValue> {
        let opfs_root = get_opfs_root()
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to get OPFS root: {:?}", e)))?;

        let app_dir = DirectoryHandle::from(opfs_root);

        let options = GetDirectoryHandleOptions { create: true };
        let root = app_dir
            .get_directory_handle_with_options(root_name, &options)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to create root directory: {:?}", e)))?;

        Ok(Self { root })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get or create nested directories for the parent of `path`.
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

/// Get parent directory handle for a path (without creating).
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

/// Navigate to the directory at `path` itself.
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
    val.dyn_ref::<DomException>().map(|e| e.name())
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

impl AsyncFileSystem for OpfsFileSystem {
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
            // Refuse to remove non-empty directories.
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
            // OPFS has no native rename; fall back to copy+delete for files.
            // Directory rename is not implemented (would require recursive copy).
            let meta = self.metadata(from).await?;
            if !meta.is_file() {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "OPFS rename only supports regular files",
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

            // Try as file first.
            let file_options = GetFileHandleOptions { create: false };
            if let Ok(file) = parent
                .get_file_handle_with_options(&name, &file_options)
                .await
            {
                let len = file.size().await.map(|s| s as u64).unwrap_or(0);
                return Ok(Metadata::new(FileType::file(), len, None));
            }

            // Try as directory.
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
