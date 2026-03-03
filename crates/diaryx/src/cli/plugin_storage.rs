//! CLI-specific plugin storage — file-based persistence for Extism plugins.
//!
//! Stores plugin state as files under `.diaryx/crdt/` to match the expected
//! storage layout used by the sync plugin's SQLite storage in native mode.

use std::path::{Path, PathBuf};

use diaryx_extism::PluginStorage;

/// File-based plugin storage for CLI use.
///
/// Stores binary data in `.diaryx/plugin-state/{key}.bin` files.
pub struct CliPluginStorage {
    base_dir: PathBuf,
}

impl CliPluginStorage {
    /// Create a new CLI plugin storage rooted at the given workspace.
    ///
    /// Creates the storage directory if it doesn't exist.
    pub fn new(workspace_root: &Path) -> Self {
        let base_dir = workspace_root.join(".diaryx").join("plugin-state");
        if !base_dir.exists() {
            let _ = std::fs::create_dir_all(&base_dir);
        }
        Self { base_dir }
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        // Sanitize key: replace path separators and colons with underscores
        let safe_key: String = key
            .chars()
            .map(|c| {
                if c == '/' || c == '\\' || c == ':' {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        self.base_dir.join(format!("{}.bin", safe_key))
    }
}

impl PluginStorage for CliPluginStorage {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.key_to_path(key);
        std::fs::read(&path).ok()
    }

    fn set(&self, key: &str, data: &[u8]) {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, data);
    }

    fn delete(&self, key: &str) {
        let path = self.key_to_path(key);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = CliPluginStorage::new(dir.path());

        assert!(storage.get("test-key").is_none());

        storage.set("test-key", b"hello world");
        assert_eq!(storage.get("test-key").unwrap(), b"hello world");

        storage.delete("test-key");
        assert!(storage.get("test-key").is_none());
    }

    #[test]
    fn test_key_sanitization() {
        let dir = tempfile::tempdir().unwrap();
        let storage = CliPluginStorage::new(dir.path());

        storage.set("body:ws/file.md", b"content");
        assert_eq!(storage.get("body:ws/file.md").unwrap(), b"content");
    }
}
