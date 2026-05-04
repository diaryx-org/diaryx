//! IndexedDB implementation of AsyncFileSystem.
//!
//! Uses the `indexed_db` crate to provide persistent file storage in browsers
//! that don't fully support OPFS (e.g., Safari in main thread context).
//!
//! ## Storage Schema
//!
//! - Database name: "diaryx" (or "diaryx-{name}" for named workspaces)
//! - Object stores:
//!   - `files`: text files keyed by path → `JsString`
//!   - `binary_files`: binary files keyed by path → `Uint8Array`
//!   - `directories`: explicit directory markers keyed by path → `true`
//!
//! Path normalization strips leading `./` and `/` so `"./README.md"`,
//! `"/README.md"`, and `"README.md"` map to the same key.

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use diaryx_core::fs::{AsyncFileSystem, BoxFuture, DirEntry, FileType, Metadata};
use indexed_db::{Database, Factory};
use js_sys::{JsString, Uint8Array};
use wasm_bindgen::prelude::*;

// ============================================================================
// Constants
// ============================================================================

const DB_NAME: &str = "diaryx";
const DB_VERSION: u32 = 2;
const STORE_FILES: &str = "files";
const STORE_BINARY_FILES: &str = "binary_files";
const STORE_DIRECTORIES: &str = "directories";

fn normalize_file_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    let trimmed = value.trim_start_matches("./").trim_start_matches('/');
    trimmed.to_string()
}

fn normalize_dir_input(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    if value == "." {
        return String::new();
    }
    while value.starts_with("./") {
        value = value[2..].to_string();
    }
    if value.is_empty() {
        return String::new();
    }
    while value.ends_with('/') {
        value.pop();
    }
    value
}

fn all_directory_prefixes(path: &str) -> Vec<String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut dirs = Vec::new();
    let mut current = String::new();

    for segment in segments {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        dirs.push(current.clone());
    }

    dirs
}

fn parent_directories(path: &str) -> Vec<String> {
    let mut dirs = all_directory_prefixes(path);
    let _ = dirs.pop();
    dirs
}

// ============================================================================
// IndexedDbFileSystem Implementation
// ============================================================================

#[wasm_bindgen]
pub struct IndexedDbFileSystem {
    db: Rc<Database<Error>>,
}

impl Clone for IndexedDbFileSystem {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}

fn idb_to_io_error(e: indexed_db::Error<Error>) -> Error {
    use indexed_db::Error as I;
    let kind = match &e {
        I::User(_) => {
            return match e {
                I::User(inner) => inner,
                _ => unreachable!(),
            };
        }
        I::AlreadyExists => ErrorKind::AlreadyExists,
        I::DoesNotExist | I::ObjectStoreWasRemoved => ErrorKind::NotFound,
        I::ReadOnly => ErrorKind::ReadOnlyFilesystem,
        I::OperationNotAllowed => ErrorKind::PermissionDenied,
        I::OperationNotSupported | I::NotInBrowser | I::IndexedDbDisabled | I::DatabaseIsClosed => {
            ErrorKind::Unsupported
        }
        I::InvalidKey
        | I::InvalidArgument
        | I::InvalidRange
        | I::InvalidCall
        | I::VersionMustNotBeZero
        | I::VersionTooOld => ErrorKind::InvalidInput,
        _ => ErrorKind::Other,
    };
    Error::new(kind, format!("{:?}", e))
}

#[wasm_bindgen]
impl IndexedDbFileSystem {
    /// Create a new IndexedDbFileSystem with the default database name.
    #[wasm_bindgen]
    pub async fn create() -> std::result::Result<IndexedDbFileSystem, JsValue> {
        Self::create_with_name(None).await
    }

    /// Create a new IndexedDbFileSystem with an optional custom database name.
    ///
    /// When `db_name` is provided, uses `"diaryx-{db_name}"` as the database name,
    /// allowing multiple isolated IndexedDB databases (one per workspace).
    /// When `None`, uses the legacy `"diaryx"` database name.
    #[wasm_bindgen(js_name = "createWithName")]
    pub async fn create_with_name(
        db_name: Option<String>,
    ) -> std::result::Result<IndexedDbFileSystem, JsValue> {
        let name = match db_name {
            Some(ref n) if !n.is_empty() => format!("diaryx-{}", n),
            _ => DB_NAME.to_string(),
        };

        let factory = Factory::<Error>::get()
            .map_err(|e| JsValue::from_str(&format!("Failed to get IndexedDB factory: {:?}", e)))?;

        let db = factory
            .open(&name, DB_VERSION, |evt| async move {
                let db = evt.database();

                if !db.object_store_names().contains(&STORE_FILES.to_string()) {
                    db.build_object_store(STORE_FILES).create()?;
                }

                if !db
                    .object_store_names()
                    .contains(&STORE_BINARY_FILES.to_string())
                {
                    db.build_object_store(STORE_BINARY_FILES).create()?;
                }

                if !db
                    .object_store_names()
                    .contains(&STORE_DIRECTORIES.to_string())
                {
                    db.build_object_store(STORE_DIRECTORIES).create()?;
                }

                Ok(())
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to open IndexedDB: {:?}", e)))?;

        Ok(Self { db: Rc::new(db) })
    }
}

// ============================================================================
// AsyncFileSystem Implementation
// ============================================================================

impl AsyncFileSystem for IndexedDbFileSystem {
    fn read<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let path_str = normalize_file_path(path);
        let db = self.db.clone();

        Box::pin(async move {
            let path_for_err = path_str.clone();
            let result = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES])
                .run(move |t| async move {
                    // Try binary first.
                    let bin = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    if let Some(value) = bin.get(&key).await? {
                        if let Some(arr) = value.dyn_ref::<Uint8Array>() {
                            return Ok(Some(arr.to_vec()));
                        }
                    }
                    // Fall back to text.
                    let files = t.object_store(STORE_FILES)?;
                    if let Some(value) = files.get(&key).await? {
                        if let Some(s) = value.dyn_ref::<JsString>() {
                            return Ok(Some(String::from(s).into_bytes()));
                        }
                    }
                    Ok(None)
                })
                .await
                .map_err(idb_to_io_error)?;

            result.ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("File not found: {}", path_for_err),
                )
            })
        })
    }

    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let path_str = normalize_file_path(path);
        let db = self.db.clone();

        Box::pin(async move {
            let path_for_err = path_str.clone();
            let result = db
                .transaction(&[STORE_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = store.get(&key).await?;
                    Ok(value)
                })
                .await
                .map_err(idb_to_io_error)?;

            match result {
                Some(value) => {
                    let js_str: JsString = value
                        .dyn_into()
                        .map_err(|_| Error::new(ErrorKind::InvalidData, "Value is not a string"))?;
                    Ok(String::from(&js_str))
                }
                None => Err(Error::new(
                    ErrorKind::NotFound,
                    format!("File not found: {}", path_for_err),
                )),
            }
        })
    }

    fn read_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<DirEntry>>> {
        let dir_str = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            let prefix = if dir_str.is_empty() {
                String::new()
            } else {
                format!("{}/", dir_str)
            };

            let (file_keys, binary_keys, directory_keys) = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .run(move |t| async move {
                    let mut files = Vec::new();
                    let mut binaries = Vec::new();
                    let mut dirs = Vec::new();

                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            files.push(String::from(s));
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            binaries.push(String::from(s));
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_DIRECTORIES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            dirs.push(String::from(s));
                        }
                        cursor.advance(1).await?;
                    }

                    Ok((files, binaries, dirs))
                })
                .await
                .map_err(idb_to_io_error)?;

            // Build a map of direct-child name → file_type.
            // Files take precedence over directories of the same name (shouldn't happen in practice).
            let mut entries: std::collections::HashMap<String, FileType> =
                std::collections::HashMap::new();

            let mut record_file = |key: &str| {
                let rest = if prefix.is_empty() {
                    key
                } else if let Some(r) = key.strip_prefix(&prefix) {
                    r
                } else {
                    return;
                };
                if rest.is_empty() {
                    return;
                }
                if let Some(slash_pos) = rest.find('/') {
                    let dir_name = &rest[..slash_pos];
                    entries
                        .entry(dir_name.to_string())
                        .or_insert(FileType::dir());
                } else {
                    entries.insert(rest.to_string(), FileType::file());
                }
            };

            for k in &file_keys {
                record_file(k);
            }
            for k in &binary_keys {
                record_file(k);
            }

            // Explicit directories may live under prefix; surface them as direct children.
            for k in &directory_keys {
                let rest = if prefix.is_empty() {
                    k.as_str()
                } else if let Some(r) = k.strip_prefix(&prefix) {
                    r
                } else {
                    continue;
                };
                if rest.is_empty() {
                    continue;
                }
                let direct = match rest.find('/') {
                    Some(pos) => &rest[..pos],
                    None => rest,
                };
                entries.entry(direct.to_string()).or_insert(FileType::dir());
            }

            let result: Vec<DirEntry> = entries
                .into_iter()
                .map(|(name, ft)| {
                    let full = if prefix.is_empty() {
                        PathBuf::from(name)
                    } else {
                        PathBuf::from(format!("{}{}", prefix, name))
                    };
                    DirEntry::new(full, ft)
                })
                .collect();

            Ok(result)
        })
    }

    fn write<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let contents = contents.to_vec();
        let db = self.db.clone();
        let parent_dirs = parent_directories(&path_str);

        Box::pin(async move {
            // Prefer text storage when contents are valid UTF-8 (preserves
            // round-trip with read_to_string). Otherwise store as binary.
            let stored_as_text = std::str::from_utf8(&contents).is_ok();

            db.transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let key = JsString::from(path_str.as_str());

                    let files = t.object_store(STORE_FILES)?;
                    let bin = t.object_store(STORE_BINARY_FILES)?;

                    if stored_as_text {
                        // Safe: stored_as_text was derived from a successful from_utf8.
                        let s = std::str::from_utf8(&contents).expect("checked above");
                        files.put_kv(&key, &JsString::from(s)).await?;
                        // Drop any prior binary entry at the same key.
                        bin.delete(&key).await?;
                    } else {
                        let array = Uint8Array::from(contents.as_slice());
                        bin.put_kv(&key, &array).await?;
                        files.delete(&key).await?;
                    }

                    let dir_store = t.object_store(STORE_DIRECTORIES)?;
                    for dir in &parent_dirs {
                        let dir_key = JsString::from(dir.as_str());
                        dir_store.put_kv(&dir_key, &JsValue::TRUE).await?;
                    }

                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn create_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let dir_path = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            if dir_path.is_empty() {
                return Ok(());
            }
            db.transaction(&[STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_DIRECTORIES)?;
                    let key = JsString::from(dir_path.as_str());
                    store.put_kv(&key, &JsValue::TRUE).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn create_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let dir_path = normalize_dir_input(path);
        let db = self.db.clone();
        let dirs = all_directory_prefixes(&dir_path);

        Box::pin(async move {
            if dirs.is_empty() {
                return Ok(());
            }

            db.transaction(&[STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_DIRECTORIES)?;
                    for dir in &dirs {
                        let key = JsString::from(dir.as_str());
                        store.put_kv(&key, &JsValue::TRUE).await?;
                    }
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn remove_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let db = self.db.clone();

        Box::pin(async move {
            db.transaction(&[STORE_FILES, STORE_BINARY_FILES])
                .rw()
                .run(move |t| async move {
                    let key = JsString::from(path_str.as_str());

                    let files = t.object_store(STORE_FILES)?;
                    let binaries = t.object_store(STORE_BINARY_FILES)?;
                    let text_exists = files.get(&key).await?.is_some();
                    let binary_exists = binaries.get(&key).await?.is_some();

                    if !text_exists && !binary_exists {
                        return Err(indexed_db::Error::User(Error::new(
                            ErrorKind::NotFound,
                            format!("File not found: {}", path_str),
                        )));
                    }

                    if text_exists {
                        files.delete(&key).await?;
                    }
                    if binary_exists {
                        binaries.delete(&key).await?;
                    }

                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
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
            let dir_str = normalize_dir_input(path);
            if dir_str.is_empty() {
                return Ok(());
            }
            let db = self.db.clone();
            db.transaction(&[STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_DIRECTORIES)?;
                    let key = JsString::from(dir_str.as_str());
                    store.delete(&key).await?;
                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn remove_dir_all<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
        let dir_str = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            let prefix = if dir_str.is_empty() {
                String::new()
            } else {
                format!("{}/", dir_str)
            };
            let dir_str_for_dirs = dir_str.clone();

            db.transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    // Collect-then-delete for each store (cursor + delete in one pass is
                    // not supported by the indexed_db crate's Send-bounded cursor).
                    let files = t.object_store(STORE_FILES)?;
                    let mut to_remove = Vec::new();
                    let mut cursor = files.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            let k = String::from(s);
                            if prefix.is_empty() || k.starts_with(&prefix) {
                                to_remove.push(JsString::from(k.as_str()));
                            }
                        }
                        cursor.advance(1).await?;
                    }
                    for k in &to_remove {
                        files.delete(k).await?;
                    }

                    let binaries = t.object_store(STORE_BINARY_FILES)?;
                    let mut to_remove = Vec::new();
                    let mut cursor = binaries.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            let k = String::from(s);
                            if prefix.is_empty() || k.starts_with(&prefix) {
                                to_remove.push(JsString::from(k.as_str()));
                            }
                        }
                        cursor.advance(1).await?;
                    }
                    for k in &to_remove {
                        binaries.delete(k).await?;
                    }

                    let dirs = t.object_store(STORE_DIRECTORIES)?;
                    let mut to_remove = Vec::new();
                    let mut cursor = dirs.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            let k = String::from(s);
                            if k == dir_str_for_dirs
                                || (!prefix.is_empty() && k.starts_with(&prefix))
                            {
                                to_remove.push(JsString::from(k.as_str()));
                            }
                        }
                        cursor.advance(1).await?;
                    }
                    for k in &to_remove {
                        dirs.delete(k).await?;
                    }

                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let meta = self.metadata(from).await?;
            if !meta.is_file() {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "IndexedDB rename only supports regular files",
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
        let path_str = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            if path_str.is_empty() {
                return Ok(Metadata::new(FileType::dir(), 0, None));
            }
            let path_for_err = path_str.clone();

            let result = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .run(move |t| async move {
                    let key = JsString::from(path_str.as_str());

                    // Text file.
                    let files = t.object_store(STORE_FILES)?;
                    if let Some(value) = files.get(&key).await? {
                        if let Some(s) = value.dyn_ref::<JsString>() {
                            let len = String::from(s).len() as u64;
                            return Ok(Some((FileType::file(), len)));
                        }
                    }

                    // Binary file.
                    let bin = t.object_store(STORE_BINARY_FILES)?;
                    if let Some(value) = bin.get(&key).await? {
                        if let Some(arr) = value.dyn_ref::<Uint8Array>() {
                            return Ok(Some((FileType::file(), arr.length() as u64)));
                        }
                    }

                    // Explicit directory marker.
                    let dirs = t.object_store(STORE_DIRECTORIES)?;
                    if dirs.get(&key).await?.is_some() {
                        return Ok(Some((FileType::dir(), 0)));
                    }

                    // Legacy fallback: directory inferred from a file prefix.
                    let prefix = format!("{}/", path_str);
                    let mut cursor = files.cursor().open().await?;
                    while let Some(file_key) = cursor.key() {
                        if let Some(s) = file_key.dyn_ref::<JsString>() {
                            if String::from(s).starts_with(&prefix) {
                                return Ok(Some((FileType::dir(), 0)));
                            }
                        }
                        cursor.advance(1).await?;
                    }
                    let mut cursor = bin.cursor().open().await?;
                    while let Some(file_key) = cursor.key() {
                        if let Some(s) = file_key.dyn_ref::<JsString>() {
                            if String::from(s).starts_with(&prefix) {
                                return Ok(Some((FileType::dir(), 0)));
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(None)
                })
                .await
                .map_err(idb_to_io_error)?;

            match result {
                Some((ft, len)) => Ok(Metadata::new(ft, len, None)),
                None => Err(Error::new(
                    ErrorKind::NotFound,
                    format!("Path not found: {}", path_for_err),
                )),
            }
        })
    }

    fn create_new<'a>(&'a self, path: &'a Path, contents: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let contents = contents.to_vec();
        let db = self.db.clone();
        let parent_dirs = parent_directories(&path_str);

        Box::pin(async move {
            let stored_as_text = std::str::from_utf8(&contents).is_ok();

            db.transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let files = t.object_store(STORE_FILES)?;
                    let bin = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());

                    if files.get(&key).await?.is_some() || bin.get(&key).await?.is_some() {
                        return Err(indexed_db::Error::User(Error::new(
                            ErrorKind::AlreadyExists,
                            format!("File already exists: {}", path_str),
                        )));
                    }

                    if stored_as_text {
                        let s = std::str::from_utf8(&contents).expect("checked above");
                        files.put_kv(&key, &JsString::from(s)).await?;
                    } else {
                        let array = Uint8Array::from(contents.as_slice());
                        bin.put_kv(&key, &array).await?;
                    }

                    let dir_store = t.object_store(STORE_DIRECTORIES)?;
                    for dir in &parent_dirs {
                        let dir_key = JsString::from(dir.as_str());
                        dir_store.put_kv(&dir_key, &JsValue::TRUE).await?;
                    }

                    Ok(())
                })
                .await
                .map_err(idb_to_io_error)
        })
    }
}
