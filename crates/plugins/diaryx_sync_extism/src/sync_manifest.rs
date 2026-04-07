//! Local sync manifest tracking file state and pending operations.
//!
//! The manifest is persisted via host storage as JSON and records which files
//! are clean (in sync with the server) or dirty (modified locally since last
//! sync), along with pending delete records for files removed locally.

use std::collections::BTreeMap;

use diaryx_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};

const MANIFEST_STORAGE_KEY: &str = "sync_manifest";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    pub namespace_id: String,
    pub files: BTreeMap<String, FileEntry>,
    pub last_sync_at: Option<u64>,
    pub pending_deletes: Vec<DeleteRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_at: u64,
    pub state: SyncState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    Clean,
    Dirty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteRecord {
    pub path: String,
    pub deleted_at: u64,
}

impl SyncManifest {
    pub fn new(namespace_id: String) -> Self {
        Self {
            namespace_id,
            files: BTreeMap::new(),
            last_sync_at: None,
            pending_deletes: Vec::new(),
        }
    }

    pub fn load(namespace_id: &str) -> Self {
        match host::storage::get(MANIFEST_STORAGE_KEY) {
            Ok(Some(data)) => match serde_json::from_slice::<SyncManifest>(&data) {
                Ok(manifest) if manifest.namespace_id == namespace_id => manifest,
                _ => Self::new(namespace_id.to_string()),
            },
            _ => Self::new(namespace_id.to_string()),
        }
    }

    pub fn save(&self) {
        if let Ok(data) = serde_json::to_vec(self) {
            let _ = host::storage::set(MANIFEST_STORAGE_KEY, &data);
        }
    }

    pub fn mark_dirty(&mut self, path: &str) {
        if let Some(entry) = self.files.get_mut(path) {
            entry.state = SyncState::Dirty;
        } else {
            // File not yet tracked — will be picked up during scan
            // We insert a placeholder that scan will update with the real hash
            self.files.insert(
                path.to_string(),
                FileEntry {
                    content_hash: String::new(),
                    size_bytes: 0,
                    modified_at: host::time::timestamp_millis().unwrap_or(0) as u64,
                    state: SyncState::Dirty,
                },
            );
        }
    }

    pub fn mark_clean(&mut self, path: &str, hash: &str, size: u64, modified_at: u64) {
        self.files.insert(
            path.to_string(),
            FileEntry {
                content_hash: hash.to_string(),
                size_bytes: size,
                modified_at,
                state: SyncState::Clean,
            },
        );
    }

    pub fn record_delete(&mut self, path: &str) {
        self.files.remove(path);
        let already_pending = self.pending_deletes.iter().any(|d| d.path == path);
        if !already_pending {
            self.pending_deletes.push(DeleteRecord {
                path: path.to_string(),
                deleted_at: host::time::timestamp_millis().unwrap_or(0) as u64,
            });
        }
    }

    pub fn clear_deletes(&mut self) {
        self.pending_deletes.clear();
    }

    pub fn dirty_count(&self) -> usize {
        self.files
            .values()
            .filter(|e| e.state == SyncState::Dirty)
            .count()
    }

    pub fn clean_count(&self) -> usize {
        self.files
            .values()
            .filter(|e| e.state == SyncState::Clean)
            .count()
    }
}
