//! Platform-agnostic sync document manager.
//!
//! Extracts the transport-independent document operations from the siphonophore
//! hooks into a reusable component. Both the native server (via `DiarySyncHook`)
//! and the Cloudflare DO adapter use this for document load/save/change.

use crate::crdt_storage::{CrdtStorage, StorageResult, UpdateOrigin};
use crate::protocol_types::{ManifestFileEntry, select_persistable_update};
use crate::workspace_doc::WorkspaceCrdt;
use std::sync::Arc;
use tracing::{debug, info};
use yrs::{Doc, ReadTxn, Transact, Update, updates::decoder::Decode};

/// Platform-agnostic sync document manager.
///
/// Handles document load, save, change persistence, and handshake completion
/// using the [`CrdtStorage`] trait. No transport assumptions — works with
/// siphonophore, Durable Objects, or any other WebSocket layer.
pub struct SyncDocManager {
    storage: Arc<dyn CrdtStorage>,
}

impl SyncDocManager {
    pub fn new(storage: Arc<dyn CrdtStorage>) -> Self {
        Self { storage }
    }

    /// Load a document by storage key, merging base snapshot + incremental updates.
    ///
    /// Returns `None` if the document doesn't exist.
    pub fn load_document(&self, storage_key: &str) -> StorageResult<Option<Vec<u8>>> {
        let base_state = self.storage.load_doc(storage_key)?;

        let updates = match self.storage.get_all_updates(storage_key) {
            Ok(u) => u,
            Err(e) => {
                debug!("No incremental updates for {}: {}", storage_key, e);
                Vec::new()
            }
        };

        if base_state.is_none() && updates.is_empty() {
            return Ok(None);
        }

        // Merge base + incremental updates into a single state
        let doc = Doc::new();
        {
            let mut txn = doc.transact_mut();
            if let Some(state) = &base_state {
                if let Ok(update) = Update::decode_v1(state) {
                    let _ = txn.apply_update(update);
                }
            }
            for crdt_update in &updates {
                if let Ok(update) = Update::decode_v1(&crdt_update.data) {
                    let _ = txn.apply_update(update);
                }
            }
        }
        let merged = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());

        debug!(
            "Loaded document {}: {} bytes (base={}, updates={})",
            storage_key,
            merged.len(),
            base_state.as_ref().map(|s| s.len()).unwrap_or(0),
            updates.len()
        );
        Ok(Some(merged))
    }

    /// Persist an incoming update with Y-sync framing detection.
    ///
    /// Strips Y-sync protocol framing if present, then stores the raw Y update.
    pub fn apply_change(
        &self,
        storage_key: &str,
        update: &[u8],
        origin: UpdateOrigin,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> StorageResult<()> {
        let (update_data, update_mode) = select_persistable_update(update);

        self.storage.append_update_with_device(
            storage_key,
            update_data.as_ref(),
            origin,
            device_id,
            device_name,
        )?;

        debug!(
            "Persisted {} byte update for {} (mode={})",
            update_data.len(),
            storage_key,
            update_mode
        );
        Ok(())
    }

    /// Save a full document snapshot (compaction).
    pub fn save_document(&self, storage_key: &str, state: &[u8]) -> StorageResult<()> {
        self.storage.save_doc(storage_key, state)?;
        info!("Saved document {} ({} bytes)", storage_key, state.len());
        Ok(())
    }

    /// Generate file manifest entries from the workspace CRDT state.
    ///
    /// Loads the workspace document via `WorkspaceCrdt` and returns manifest
    /// entries for active (non-deleted) files.
    pub fn generate_file_manifest(
        &self,
        namespace_id: &str,
    ) -> StorageResult<Vec<ManifestFileEntry>> {
        let doc_name = format!("workspace:{}", namespace_id);
        let workspace = WorkspaceCrdt::load_with_name(self.storage.clone(), doc_name)?;

        Ok(workspace
            .list_active_files()
            .into_iter()
            .map(|(path, meta)| ManifestFileEntry {
                doc_id: format!("body:{}/{}", namespace_id, path),
                filename: meta.filename,
                title: None,
                part_of: meta.part_of,
                deleted: false,
            })
            .collect())
    }

    /// Complete the Files-Ready handshake.
    ///
    /// Returns JSON text messages to send to the client:
    /// 1. `crdt_state` with the workspace CRDT state (base64 encoded)
    /// 2. `sync_complete` with the file count
    pub fn complete_handshake(&self, namespace_id: &str) -> StorageResult<Vec<String>> {
        let workspace_key = format!("workspace:{}", namespace_id);
        let mut messages = Vec::new();

        let files_synced = self.generate_file_manifest(namespace_id)?.len();

        if let Some(state) = self.storage.load_doc(&workspace_key)? {
            let state_b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &state);
            let crdt_state = serde_json::json!({
                "type": "crdt_state",
                "state": state_b64
            });
            messages.push(crdt_state.to_string());
            info!(
                "Completing handshake with CRDT state ({} bytes)",
                state.len()
            );
        }

        let sync_complete = serde_json::json!({
            "type": "sync_complete",
            "files_synced": files_synced
        });
        messages.push(sync_complete.to_string());

        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use yrs::{GetString, Text};

    #[test]
    fn test_load_empty_document() {
        let storage = Arc::new(MemoryStorage::new());
        let manager = SyncDocManager::new(storage);
        let result = manager.load_document("workspace:test").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_save_and_load_document() {
        let storage = Arc::new(MemoryStorage::new());
        let manager = SyncDocManager::new(storage);

        // Create a Y.Doc and encode its state
        let doc = Doc::new();
        {
            let text = doc.get_or_insert_text("body");
            let mut txn = doc.transact_mut();
            text.insert(&mut txn, 0, "Hello, world!");
        }
        let state = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());

        manager.save_document("workspace:test", &state).unwrap();

        let loaded = manager.load_document("workspace:test").unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn test_apply_change() {
        let storage = Arc::new(MemoryStorage::new());
        let manager = SyncDocManager::new(storage.clone());

        // Create initial state
        let doc = Doc::new();
        let state = doc
            .transact()
            .encode_state_as_update_v1(&yrs::StateVector::default());

        manager
            .apply_change("workspace:test", &state, UpdateOrigin::Local, None, None)
            .unwrap();

        let updates = storage.get_all_updates("workspace:test").unwrap();
        assert_eq!(updates.len(), 1);
    }

    #[test]
    fn test_complete_handshake_empty() {
        let storage = Arc::new(MemoryStorage::new());
        let manager = SyncDocManager::new(storage);

        let messages = manager.complete_handshake("test-ns").unwrap();
        // Should have sync_complete even with no state
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("sync_complete"));
    }
}
