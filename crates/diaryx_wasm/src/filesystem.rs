//! Filesystem operations for WASM (IndexedDB sync).

use diaryx_core::fs::{FileSystem, InMemoryFileSystem};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::error::IntoJsResult;
use crate::state::{with_fs, with_fs_mut};

// ============================================================================  
// Types
// ============================================================================

#[derive(Serialize, Deserialize)]
struct BinaryEntry {
    path: String,
    data: Vec<u8>,
}

/// Result of a backup operation
#[derive(Serialize)]
pub struct JsBackupResult {
    pub success: bool,
    pub files_processed: usize,
    pub text_files: usize,
    pub binary_files: usize,
    pub error: Option<String>,
}

// ============================================================================
// DiaryxFilesystem Class
// ============================================================================

/// Filesystem operations for IndexedDB sync.
#[wasm_bindgen]
pub struct DiaryxFilesystem;

#[wasm_bindgen]
impl DiaryxFilesystem {
    /// Create a new DiaryxFilesystem instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Load files into the in-memory filesystem from JavaScript.
    #[wasm_bindgen]
    pub fn load_files(&self, entries: JsValue) -> Result<(), JsValue> {
        use crate::state::replace_fs;
        let entries: Vec<(String, String)> = serde_wasm_bindgen::from_value(entries).js_err()?;
        replace_fs(InMemoryFileSystem::load_from_entries(entries));
        Ok(())
    }

    /// Export all files from the in-memory filesystem.
    #[wasm_bindgen]
    pub fn export_files(&self) -> Result<JsValue, JsValue> {
        let entries = with_fs(|fs| fs.export_entries());
        serde_wasm_bindgen::to_value(&entries).js_err()
    }

    /// Export all binary files from the in-memory filesystem.
    #[wasm_bindgen]
    pub fn export_binary_files(&self) -> Result<JsValue, JsValue> {
        let entries = with_fs(|fs| fs.export_binary_entries());
        let serializable: Vec<BinaryEntry> = entries
            .into_iter()
            .map(|(path, data)| BinaryEntry { path, data })
            .collect();
        serde_wasm_bindgen::to_value(&serializable).js_err()
    }

    /// Load binary files into the in-memory filesystem.
    #[wasm_bindgen]
    pub fn load_binary_files(&self, entries: JsValue) -> Result<(), JsValue> {
        let binary_entries: Vec<BinaryEntry> = serde_wasm_bindgen::from_value(entries).js_err()?;
        let entries: Vec<(String, Vec<u8>)> = binary_entries
            .into_iter()
            .map(|e| (e.path, e.data))
            .collect();

        with_fs_mut(|fs| {
            fs.load_binary_entries(entries);
        });

        Ok(())
    }

    /// Check if a file exists.
    #[wasm_bindgen]
    pub fn file_exists(&self, path: &str) -> bool {
        with_fs(|fs| FileSystem::exists(fs, std::path::Path::new(path)))
    }

    /// Read a file's content.
    #[wasm_bindgen]
    pub fn read_file(&self, path: &str) -> Result<String, JsValue> {
        with_fs(|fs| fs.read_to_string(std::path::Path::new(path)).js_err())
    }

    /// Write content to a file.
    #[wasm_bindgen]
    pub fn write_file(&self, path: &str, content: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| fs.write_file(std::path::Path::new(path), content).js_err())
    }

    /// Delete a file.
    #[wasm_bindgen]
    pub fn delete_file(&self, path: &str) -> Result<(), JsValue> {
        with_fs_mut(|fs| fs.delete_file(std::path::Path::new(path)).js_err())
    }

    /// Get backup data for persistence to IndexedDB.
    #[wasm_bindgen]
    pub fn get_backup_data(&self) -> Result<JsValue, JsValue> {
        let text_entries = with_fs(|fs| fs.export_entries());
        let binary_entries = with_fs(|fs| fs.export_binary_entries());

        #[derive(Serialize)]
        struct BackupData {
            text_files: Vec<(String, String)>,
            binary_files: Vec<BinaryEntry>,
            text_count: usize,
            binary_count: usize,
        }

        let binary_files: Vec<BinaryEntry> = binary_entries
            .into_iter()
            .map(|(path, data)| BinaryEntry { path, data })
            .collect();

        let data = BackupData {
            text_count: text_entries.len(),
            binary_count: binary_files.len(),
            text_files: text_entries,
            binary_files,
        };

        serde_wasm_bindgen::to_value(&data).js_err()
    }

    /// Restore from backup data.
    #[wasm_bindgen]
    pub fn restore_from_backup(&self, data: JsValue) -> Result<JsValue, JsValue> {
        #[derive(Deserialize)]
        struct BackupData {
            text_files: Vec<(String, String)>,
            binary_files: Vec<BinaryEntry>,
        }

        let backup: BackupData = serde_wasm_bindgen::from_value(data).js_err()?;

        use crate::state::replace_fs;
        replace_fs(InMemoryFileSystem::load_from_entries(backup.text_files.clone()));

        let binary_entries: Vec<(String, Vec<u8>)> = backup
            .binary_files
            .iter()
            .map(|e| (e.path.clone(), e.data.clone()))
            .collect();

        with_fs_mut(|fs| {
            fs.load_binary_entries(binary_entries);
        });

        let result = JsBackupResult {
            success: true,
            files_processed: backup.text_files.len() + backup.binary_files.len(),
            text_files: backup.text_files.len(),
            binary_files: backup.binary_files.len(),
            error: None,
        };

        serde_wasm_bindgen::to_value(&result).js_err()
    }
}

impl Default for DiaryxFilesystem {
    fn default() -> Self {
        Self::new()
    }
}
