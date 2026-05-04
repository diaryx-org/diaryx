//! JavaScript-backed AsyncFileSystem implementation.
//!
//! Implements `AsyncFileSystem` by delegating each operation to a JavaScript
//! callback. The callback names mirror `std::fs` / `tokio::fs`: `read`,
//! `readToString`, `readDir`, `write`, `createDir`, `createDirAll`,
//! `removeFile`, `removeDir`, `removeDirAll`, `rename`, `metadata`,
//! `createNew`.
//!
//! ## Usage from JavaScript
//!
//! ```javascript
//! import { JsAsyncFileSystem } from './wasm/diaryx_wasm.js';
//!
//! const fs = new JsAsyncFileSystem({
//!   read:           async (path)         => new Uint8Array(await db.get(path)),
//!   readToString:   async (path)         => await db.getText(path),
//!   readDir:        async (path)         => [{ name: 'a.md', kind: 'file' }],
//!   write:          async (path, bytes)  => await db.put(path, bytes),
//!   createDir:      async (path)         => await db.mkdir(path),
//!   createDirAll:   async (path)         => await db.mkdirp(path),
//!   removeFile:     async (path)         => await db.unlink(path),
//!   removeDir:      async (path)         => await db.rmdir(path),
//!   removeDirAll:   async (path)         => await db.rmRf(path),
//!   rename:         async (from, to)     => await db.rename(from, to),
//!   metadata:       async (path)         => ({ kind: 'file', len: 42 }),
//!   createNew:      async (path, bytes)  => await db.create(path, bytes),
//! });
//! ```

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use diaryx_core::fs::crossfs;
use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

// ============================================================================
// JsAsyncFileSystem
// ============================================================================

/// `AsyncFileSystem` impl backed by JavaScript callbacks.
#[wasm_bindgen]
#[derive(Clone)]
pub struct JsAsyncFileSystem {
    callbacks: JsValue,
}

#[wasm_bindgen]
impl JsAsyncFileSystem {
    /// Create a new JsAsyncFileSystem with the provided callbacks.
    ///
    /// The callbacks object should implement the `JsFileSystemCallbacks` interface.
    #[wasm_bindgen(constructor)]
    pub fn new(callbacks: JsValue) -> Self {
        Self { callbacks }
    }

    /// True if the callbacks object provides a function with the given name.
    #[wasm_bindgen]
    pub fn has_callback(&self, name: &str) -> bool {
        get_callback(&self.callbacks, name).is_some()
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert a JS error into an `io::Error`.
///
/// The kind is inferred (in priority order):
/// 1. An explicit `kind` property on the thrown object, matching one of the
///    canonical io::ErrorKind names ("NotFound", "AlreadyExists",
///    "PermissionDenied", "Unsupported", "InvalidInput", "InvalidData",
///    "QuotaExceeded", "ReadOnlyFilesystem", "DirectoryNotEmpty",
///    "IsADirectory", "NotADirectory", "Interrupted", "TimedOut").
/// 2. A `name` property matching a `DOMException` name (e.g. `NotFoundError`),
///    via [`crossfs::error::dom_exception_kind`].
/// 3. Otherwise, [`ErrorKind::Other`].
fn js_to_io_error(err: JsValue) -> Error {
    let kind = if let Some(s) = Reflect::get(&err, &JsValue::from_str("kind"))
        .ok()
        .and_then(|v| v.as_string())
    {
        kind_from_str(&s).unwrap_or(ErrorKind::Other)
    } else if let Some(name) = Reflect::get(&err, &JsValue::from_str("name"))
        .ok()
        .and_then(|v| v.as_string())
    {
        crossfs::error::dom_exception_kind(&name)
    } else {
        ErrorKind::Other
    };

    let msg = if let Some(s) = err.as_string() {
        s
    } else if let Some(s) = Reflect::get(&err, &JsValue::from_str("message"))
        .ok()
        .and_then(|v| v.as_string())
    {
        s
    } else if let Some(obj) = err.dyn_ref::<js_sys::Object>() {
        obj.to_string()
            .as_string()
            .unwrap_or_else(|| "Unknown JS error".to_string())
    } else {
        "Unknown JS error".to_string()
    };
    Error::new(kind, msg)
}

fn kind_from_str(s: &str) -> Option<ErrorKind> {
    Some(match s {
        "NotFound" => ErrorKind::NotFound,
        "AlreadyExists" => ErrorKind::AlreadyExists,
        "PermissionDenied" => ErrorKind::PermissionDenied,
        "Unsupported" => ErrorKind::Unsupported,
        "InvalidInput" => ErrorKind::InvalidInput,
        "InvalidData" => ErrorKind::InvalidData,
        "QuotaExceeded" => ErrorKind::QuotaExceeded,
        "StorageFull" => ErrorKind::StorageFull,
        "ReadOnlyFilesystem" => ErrorKind::ReadOnlyFilesystem,
        "DirectoryNotEmpty" => ErrorKind::DirectoryNotEmpty,
        "IsADirectory" => ErrorKind::IsADirectory,
        "NotADirectory" => ErrorKind::NotADirectory,
        "Interrupted" => ErrorKind::Interrupted,
        "TimedOut" => ErrorKind::TimedOut,
        "WriteZero" => ErrorKind::WriteZero,
        "InvalidFilename" => ErrorKind::InvalidFilename,
        "FileTooLarge" => ErrorKind::FileTooLarge,
        _ => return None,
    })
}

fn get_callback(callbacks: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(callbacks, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.dyn_into::<Function>().ok())
}

async fn call_async(callbacks: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue> {
    let cb = get_callback(callbacks, name).ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            format!("Callback '{}' not provided", name),
        )
    })?;

    let this = JsValue::NULL;
    let result = match args.len() {
        0 => cb.call0(&this),
        1 => cb.call1(&this, &args[0]),
        2 => cb.call2(&this, &args[0], &args[1]),
        3 => cb.call3(&this, &args[0], &args[1], &args[2]),
        _ => {
            let js_args = Array::new();
            for a in args {
                js_args.push(a);
            }
            cb.apply(&this, &js_args)
        }
    }
    .map_err(js_to_io_error)?;

    if result.has_type::<Promise>() {
        let p: Promise = result.unchecked_into();
        JsFuture::from(p).await.map_err(js_to_io_error)
    } else {
        Ok(result)
    }
}

fn parse_uint8_array(value: JsValue) -> Result<Vec<u8>> {
    if let Some(arr) = value.dyn_ref::<Uint8Array>() {
        return Ok(arr.to_vec());
    }
    if let Some(arr) = value.dyn_ref::<Array>() {
        let mut bytes = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            bytes.push(arr.get(i).as_f64().unwrap_or(0.0) as u8);
        }
        return Ok(bytes);
    }
    Err(Error::new(
        ErrorKind::InvalidData,
        "expected Uint8Array or Array of bytes",
    ))
}

/// Parse a `readDir` result into `Vec<DirEntry>`.
///
/// Accepts either `[{ name: string, kind: 'file' | 'dir' | 'symlink' }, ...]`
/// or — for compatibility with simple JS backends — `[string, ...]` where each
/// string is a file name.
fn parse_dir_entries(value: JsValue, base: &Path) -> Result<Vec<DirEntry>> {
    let arr = value
        .dyn_ref::<Array>()
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readDir did not return an array"))?;

    let mut entries = Vec::with_capacity(arr.length() as usize);
    for i in 0..arr.length() {
        let item = arr.get(i);

        if let Some(name) = item.as_string() {
            let path = if base.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                base.join(name)
            };
            entries.push(DirEntry::new(path, FileType::file()));
            continue;
        }

        if !item.is_object() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "readDir entry must be a string or { name, kind } object",
            ));
        }

        let name = Reflect::get(&item, &JsValue::from_str("name"))
            .ok()
            .and_then(|v| v.as_string())
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "readDir entry missing 'name'"))?;
        let kind = Reflect::get(&item, &JsValue::from_str("kind"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| "file".to_string());
        let ft = match kind.as_str() {
            "dir" | "directory" => FileType::dir(),
            "symlink" | "link" => FileType::symlink(),
            _ => FileType::file(),
        };

        let path = if base.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            base.join(name)
        };
        entries.push(DirEntry::new(path, ft));
    }

    Ok(entries)
}

fn parse_metadata(value: JsValue) -> Result<Metadata> {
    if !value.is_object() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "metadata callback must return an object",
        ));
    }

    let kind = Reflect::get(&value, &JsValue::from_str("kind"))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "file".to_string());
    let ft = match kind.as_str() {
        "dir" | "directory" => FileType::dir(),
        "symlink" | "link" => FileType::symlink(),
        _ => FileType::file(),
    };

    let len = Reflect::get(&value, &JsValue::from_str("len"))
        .ok()
        .and_then(|v| v.as_f64())
        .map(|f| f as u64)
        .unwrap_or(0);

    Ok(Metadata::new(ft, len, None))
}

// ============================================================================
// AsyncFileSystem impl
// ============================================================================

impl AsyncFileSystem for JsAsyncFileSystem {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            let v = call_async(&callbacks, "read", &[JsValue::from_str(&path_str)]).await?;
            parse_uint8_array(v)
        })
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            // Prefer the dedicated callback when present; otherwise fall back to read+UTF-8.
            if get_callback(&callbacks, "readToString").is_some() {
                let v =
                    call_async(&callbacks, "readToString", &[JsValue::from_str(&path_str)]).await?;
                v.as_string().ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidData,
                        "readToString did not return a string",
                    )
                })
            } else {
                let bytes = self.read(Path::new(&path_str)).await?;
                String::from_utf8(bytes)
                    .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))
            }
        })
    }

    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            let v = call_async(&callbacks, "readDir", &[JsValue::from_str(&path_str)]).await?;
            parse_dir_entries(v, Path::new(&path_str))
        })
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let contents = contents.to_vec();
        Box::pin(async move {
            let arr = Uint8Array::new_with_length(contents.len() as u32);
            arr.copy_from(&contents);
            call_async(
                &callbacks,
                "write",
                &[JsValue::from_str(&path_str), arr.into()],
            )
            .await?;
            Ok(())
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            if get_callback(&callbacks, "createDir").is_some() {
                call_async(&callbacks, "createDir", &[JsValue::from_str(&path_str)]).await?;
            }
            Ok(())
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            if get_callback(&callbacks, "createDirAll").is_some() {
                call_async(&callbacks, "createDirAll", &[JsValue::from_str(&path_str)]).await?;
            }
            Ok(())
        })
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            call_async(&callbacks, "removeFile", &[JsValue::from_str(&path_str)]).await?;
            Ok(())
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            call_async(&callbacks, "removeDir", &[JsValue::from_str(&path_str)]).await?;
            Ok(())
        })
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            call_async(&callbacks, "removeDirAll", &[JsValue::from_str(&path_str)]).await?;
            Ok(())
        })
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();
        Box::pin(async move {
            call_async(
                &callbacks,
                "rename",
                &[JsValue::from_str(&from_str), JsValue::from_str(&to_str)],
            )
            .await?;
            Ok(())
        })
    }

    fn metadata<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Metadata>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        Box::pin(async move {
            let v = call_async(&callbacks, "metadata", &[JsValue::from_str(&path_str)]).await?;
            parse_metadata(v)
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let callbacks = self.callbacks.clone();
        let path_str = path.to_string_lossy().to_string();
        let contents = contents.to_vec();
        Box::pin(async move {
            let arr = Uint8Array::new_with_length(contents.len() as u32);
            arr.copy_from(&contents);
            if get_callback(&callbacks, "createNew").is_some() {
                call_async(
                    &callbacks,
                    "createNew",
                    &[JsValue::from_str(&path_str), arr.into()],
                )
                .await?;
                return Ok(());
            }
            // Fallback: try metadata; if it succeeds we know it exists.
            if get_callback(&callbacks, "metadata").is_some() {
                let exists = call_async(&callbacks, "metadata", &[JsValue::from_str(&path_str)])
                    .await
                    .is_ok();
                if exists {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!("File already exists: {}", path_str),
                    ));
                }
            }
            // Otherwise just write.
            let arr = Uint8Array::new_with_length(contents.len() as u32);
            arr.copy_from(&contents);
            call_async(
                &callbacks,
                "write",
                &[JsValue::from_str(&path_str), arr.into()],
            )
            .await?;
            Ok(())
        })
    }
}

// ============================================================================
// TypeScript Type Definitions
// ============================================================================

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
/**
 * Callbacks for JsAsyncFileSystem operations.
 *
 * Method names mirror std::fs / tokio::fs. All callbacks return Promises;
 * missing callbacks cause the corresponding operation to fail with
 * `ErrorKind::Unsupported` (except `createDir`, `createDirAll`, `readToString`,
 * and `createNew`, which have fallbacks).
 */
export interface JsFileSystemCallbacks {
    /** Read a file as bytes. */
    read: (path: string) => Promise<Uint8Array>;
    /** Read a file as a UTF-8 string. Optional — falls back to read + UTF-8 decode. */
    readToString?: (path: string) => Promise<string>;
    /**
     * List entries in a directory (non-recursive). Each entry is either a
     * `{ name, kind }` object (kind: 'file' | 'dir' | 'symlink') or a bare
     * string (treated as a file).
     */
    readDir: (path: string) => Promise<Array<string | { name: string; kind: 'file' | 'dir' | 'symlink' }>>;
    /** Write a file (create or overwrite). */
    write: (path: string, contents: Uint8Array) => Promise<void>;
    /** Create a single directory. Optional. */
    createDir?: (path: string) => Promise<void>;
    /** Create a directory and all parent directories. Optional. */
    createDirAll?: (path: string) => Promise<void>;
    /** Remove a regular file. */
    removeFile: (path: string) => Promise<void>;
    /** Remove an empty directory. */
    removeDir: (path: string) => Promise<void>;
    /** Recursively remove a directory and its contents. */
    removeDirAll: (path: string) => Promise<void>;
    /** Rename or move a file or directory. */
    rename: (from: string, to: string) => Promise<void>;
    /** Return metadata about a path. */
    metadata: (path: string) => Promise<{ kind: 'file' | 'dir' | 'symlink'; len?: number }>;
    /** Create a file only if it does not exist. Optional — falls back to metadata + write. */
    createNew?: (path: string, contents: Uint8Array) => Promise<void>;
}
"#;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_async_filesystem_creation() {
        let _fs = JsAsyncFileSystem::new(JsValue::NULL);
    }

    #[test]
    fn test_js_async_filesystem_clone() {
        let fs1 = JsAsyncFileSystem::new(JsValue::NULL);
        let fs2 = fs1.clone();
        assert!(!fs1.has_callback("test"));
        assert!(!fs2.has_callback("test"));
    }
}
