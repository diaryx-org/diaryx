//! IndexedDB implementation of AsyncFileSystem.
//!
//! Uses the `indexed_db` crate to provide persistent file storage in browsers
//! that don't fully support OPFS (e.g., Safari in main thread context).
//!
//! ## Storage Schema
//!
//! - Database name: "diaryx"
//! - Object stores:
//!   - "files": Text files with path as key
//!   - "binary_files": Binary files with path as key

use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use diaryx_core::fs::{AsyncFileSystem, BoxFuture};
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

/// Normalize a file path for use as an IndexedDB key.
///
/// Strips leading `./` and `/` prefixes so that `"./README.md"`, `"/README.md"`,
/// and `"README.md"` all map to the same key. This matches the normalization
/// done by `normalize_sync_path` in diaryx_core, ensuring consistency between
/// the filesystem layer and the CRDT layer.
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
    // Also strip leading "./" for consistency with file path normalization
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

/// AsyncFileSystem implementation backed by IndexedDB.
///
/// Used as a fallback for browsers that don't support OPFS or when
/// running outside a Web Worker context (where OPFS sync access isn't available).
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
    match e {
        indexed_db::Error::User(e) => e,
        other => Error::new(ErrorKind::Other, format!("{:?}", other)),
    }
}

#[wasm_bindgen]
impl IndexedDbFileSystem {
    /// Create a new IndexedDbFileSystem with the default database name.
    ///
    /// Opens or creates the IndexedDB database with the required object stores.
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

                // Create files store if it doesn't exist
                if !db.object_store_names().contains(&STORE_FILES.to_string()) {
                    db.build_object_store(STORE_FILES).create()?;
                }

                // Create binary files store if it doesn't exist
                if !db
                    .object_store_names()
                    .contains(&STORE_BINARY_FILES.to_string())
                {
                    db.build_object_store(STORE_BINARY_FILES).create()?;
                }

                // Create directories store if it doesn't exist
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
    fn read_to_string<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String>> {
        let path_str = normalize_file_path(path);
        let db = self.db.clone();

        Box::pin(async move {
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
                    format!("File not found: {}", path.display()),
                )),
            }
        })
    }

    fn write_file<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let content = content.to_string();
        let db = self.db.clone();
        let parent_dirs = parent_directories(&path_str);

        Box::pin(async move {
            db.transaction(&[STORE_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = JsString::from(content.as_str());
                    store.put_kv(&key, &value).await?;

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

    fn create_new<'a>(&'a self, path: &'a Path, content: &'a str) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let content = content.to_string();
        let db = self.db.clone();
        let parent_dirs = parent_directories(&path_str);

        Box::pin(async move {
            db.transaction(&[STORE_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());

                    // Check if file exists
                    if store.get(&key).await?.is_some() {
                        return Err(indexed_db::Error::User(Error::new(
                            ErrorKind::AlreadyExists,
                            format!("File already exists: {}", path_str),
                        )));
                    }

                    let value = JsString::from(content.as_str());
                    store.put_kv(&key, &value).await?;

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

    fn delete_file<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<()>> {
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

    fn list_md_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let dir_str = normalize_dir_input(dir_path);
        let db = self.db.clone();

        Box::pin(async move {
            let all_keys = db
                .transaction(&[STORE_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    let mut keys = Vec::new();

                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            keys.push(String::from(s));
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(keys)
                })
                .await
                .map_err(idb_to_io_error)?;

            // Filter to files in the directory
            let prefix = if dir_str.is_empty() || dir_str == "." {
                String::new()
            } else {
                format!("{}/", dir_str)
            };

            let md_files: Vec<PathBuf> = all_keys
                .into_iter()
                .filter(|key| {
                    if key.ends_with(".md") {
                        if prefix.is_empty() {
                            // Root directory - file should have no directory
                            !key.contains('/')
                        } else {
                            // Check if file is directly in the directory
                            if let Some(rest) = key.strip_prefix(&prefix) {
                                !rest.contains('/')
                            } else {
                                false
                            }
                        }
                    } else {
                        false
                    }
                })
                .map(PathBuf::from)
                .collect();

            Ok(md_files)
        })
    }

    fn exists<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        let path_str = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            if path_str.is_empty() {
                return true;
            }

            let result = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .run(move |t| async move {
                    // Check text files
                    let store = t.object_store(STORE_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    // Check binary files
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    // Check explicit directories
                    let store = t.object_store(STORE_DIRECTORIES)?;
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    // Fallback for legacy data: infer directories from file prefixes.
                    let prefix = format!("{}/", path_str);

                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(file_key) = cursor.key() {
                        if let Some(s) = file_key.dyn_ref::<JsString>()
                            && String::from(s).starts_with(&prefix)
                        {
                            return Ok(true);
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(file_key) = cursor.key() {
                        if let Some(s) = file_key.dyn_ref::<JsString>()
                            && String::from(s).starts_with(&prefix)
                        {
                            return Ok(true);
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(false)
                })
                .await;

            result.unwrap_or(false)
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

    fn is_dir<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, bool> {
        // Check if any files exist with this path as a prefix
        let dir_str = normalize_dir_input(path);
        let db = self.db.clone();

        Box::pin(async move {
            if dir_str.is_empty() {
                return true;
            }

            let result = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .run(move |t| async move {
                    // Check explicit directories first.
                    let store = t.object_store(STORE_DIRECTORIES)?;
                    let key = JsString::from(dir_str.as_str());
                    if store.get(&key).await?.is_some() {
                        return Ok(true);
                    }

                    // Fallback for legacy data without explicit directory keys.
                    let prefix = format!("{}/", dir_str);

                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;

                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            if String::from(s).starts_with(&prefix) {
                                return Ok(true);
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let mut cursor = store.cursor().open().await?;

                    while let Some(key) = cursor.key() {
                        if let Some(s) = key.dyn_ref::<JsString>() {
                            if String::from(s).starts_with(&prefix) {
                                return Ok(true);
                            }
                        }
                        cursor.advance(1).await?;
                    }

                    Ok(false)
                })
                .await;

            result.unwrap_or(false)
        })
    }

    fn move_file<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !self.exists(from).await {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("Source file not found: {}", from.display()),
                ));
            }

            if self.exists(to).await {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("Destination already exists: {}", to.display()),
                ));
            }

            let content = self.read_to_string(from).await?;
            self.write_file(to, &content).await?;
            self.delete_file(from).await?;
            Ok(())
        })
    }

    fn read_binary<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<Vec<u8>>> {
        let path_str = normalize_file_path(path);
        let db = self.db.clone();

        Box::pin(async move {
            let result = db
                .transaction(&[STORE_BINARY_FILES])
                .run(move |t| async move {
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let value = store.get(&key).await?;
                    Ok(value)
                })
                .await
                .map_err(idb_to_io_error)?;

            match result {
                Some(value) => {
                    let array: Uint8Array = value.dyn_into().map_err(|_| {
                        Error::new(ErrorKind::InvalidData, "Value is not a Uint8Array")
                    })?;
                    Ok(array.to_vec())
                }
                None => self.read_to_string(path).await.map(|s| s.into_bytes()),
            }
        })
    }

    fn write_binary<'a>(&'a self, path: &'a Path, content: &'a [u8]) -> BoxFuture<'a, Result<()>> {
        let path_str = normalize_file_path(path);
        let content = content.to_vec();
        let db = self.db.clone();
        let parent_dirs = parent_directories(&path_str);

        Box::pin(async move {
            db.transaction(&[STORE_BINARY_FILES, STORE_DIRECTORIES])
                .rw()
                .run(move |t| async move {
                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let key = JsString::from(path_str.as_str());
                    let array = Uint8Array::from(content.as_slice());
                    store.put_kv(&key, &array).await?;

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

    fn list_files<'a>(&'a self, dir_path: &'a Path) -> BoxFuture<'a, Result<Vec<PathBuf>>> {
        let dir_str = normalize_dir_input(dir_path);
        let db = self.db.clone();

        Box::pin(async move {
            let prefix = if dir_str.is_empty() {
                String::new()
            } else {
                format!("{}/", dir_str)
            };

            // Collect all relevant keys from files, binaries, and explicit directories.
            let (file_keys, binary_keys, directory_keys) = db
                .transaction(&[STORE_FILES, STORE_BINARY_FILES, STORE_DIRECTORIES])
                .run(move |t| async move {
                    let mut files = Vec::new();
                    let mut binaries = Vec::new();
                    let mut dirs = Vec::new();

                    let store = t.object_store(STORE_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(js_str) = key.dyn_ref::<JsString>() {
                            files.push(String::from(js_str));
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_BINARY_FILES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(js_str) = key.dyn_ref::<JsString>() {
                            binaries.push(String::from(js_str));
                        }
                        cursor.advance(1).await?;
                    }

                    let store = t.object_store(STORE_DIRECTORIES)?;
                    let mut cursor = store.cursor().open().await?;
                    while let Some(key) = cursor.key() {
                        if let Some(js_str) = key.dyn_ref::<JsString>() {
                            dirs.push(String::from(js_str));
                        }
                        cursor.advance(1).await?;
                    }

                    Ok((files, binaries, dirs))
                })
                .await
                .map_err(idb_to_io_error)?;

            // Extract direct children (files and directories) to match native behavior.
            let mut result = std::collections::HashSet::new();

            let mut collect_direct_children = |keys: Vec<String>, include_leaf: bool| {
                for key in keys {
                    let rest = if prefix.is_empty() {
                        key.as_str()
                    } else if let Some(r) = key.strip_prefix(&prefix) {
                        r
                    } else {
                        continue;
                    };

                    if rest.is_empty() {
                        continue;
                    }

                    if let Some(slash_pos) = rest.find('/') {
                        let dir_name = &rest[..slash_pos];
                        let full_path = if prefix.is_empty() {
                            dir_name.to_string()
                        } else {
                            format!("{}{}", prefix, dir_name)
                        };
                        result.insert(full_path);
                    } else if include_leaf {
                        result.insert(key);
                    }
                }
            };

            collect_direct_children(file_keys, true);
            collect_direct_children(binary_keys, true);
            collect_direct_children(directory_keys, true);

            Ok(result.into_iter().map(PathBuf::from).collect())
        })
    }
}
