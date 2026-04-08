//! Manifest CRDT operations using yrs Map.
//!
//! The manifest tracks the file hierarchy for a share session as a yrs Map.
//! Each entry maps a file path to its metadata (parent, size, content_type).
//! This enables guests to browse the file tree without downloading all files.

use diaryx_plugin_sdk::host;
use serde::{Deserialize, Serialize};
use yrs::types::ToJson;
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, Map, MapPrelim, MapRef, ReadTxn, StateVector, Transact, TransactionMut, Update,
};

const MANIFEST_ROOT: &str = "manifest";

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub parent: Option<String>,
    pub size: u64,
    pub content_type: String,
}

/// Build a manifest yrs Doc from the local workspace.
pub fn build_manifest_from_workspace(workspace_root: &str) -> Doc {
    let doc = Doc::new();

    let files = match host::fs::list_files(workspace_root) {
        Ok(f) => f,
        Err(_) => return doc,
    };

    {
        let map = doc.get_or_insert_map(MANIFEST_ROOT);
        let mut txn = doc.transact_mut();

        for file_path in &files {
            let relative = file_path
                .strip_prefix(workspace_root)
                .unwrap_or(file_path)
                .trim_start_matches('/');

            if relative.is_empty() {
                continue;
            }

            let parent = std::path::Path::new(relative).parent().and_then(|p| {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() { None } else { Some(s) }
            });

            let size = host::fs::read_binary(file_path)
                .map(|b| b.len() as u64)
                .unwrap_or(0);

            let content_type = if relative.ends_with(".md") {
                "text/markdown"
            } else {
                "application/octet-stream"
            };

            insert_entry(
                &map,
                &mut txn,
                relative,
                parent.as_deref(),
                size,
                content_type,
            );
        }
    }

    doc
}

fn insert_entry(
    map: &MapRef,
    txn: &mut TransactionMut,
    path: &str,
    parent: Option<&str>,
    size: u64,
    content_type: &str,
) {
    let entry = MapPrelim::from([
        ("parent".to_string(), Any::from(parent.unwrap_or(""))),
        ("size".to_string(), Any::from(size as f64)),
        ("content_type".to_string(), Any::from(content_type)),
    ]);
    map.insert(txn, path, entry);
}

/// Read all manifest entries from a yrs Doc.
#[allow(dead_code)]
pub fn read_manifest_entries(doc: &Doc) -> Vec<ManifestEntry> {
    let map = doc.get_or_insert_map(MANIFEST_ROOT);
    let txn = doc.transact();
    let mut entries = Vec::new();

    let json = map.to_json(&txn);
    if let Any::Map(map_data) = json {
        for (path, value) in map_data.iter() {
            if let Any::Map(entry_data) = value {
                let parent = entry_data.get("parent").and_then(|v| match v {
                    Any::String(s) if !s.is_empty() => Some(s.to_string()),
                    _ => None,
                });
                let size = entry_data
                    .get("size")
                    .and_then(|v| match v {
                        Any::Number(n) => Some(*n as u64),
                        _ => None,
                    })
                    .unwrap_or(0);
                let content_type = entry_data
                    .get("content_type")
                    .and_then(|v| match v {
                        Any::String(s) => Some(s.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string());

                entries.push(ManifestEntry {
                    path: path.to_string(),
                    parent,
                    size,
                    content_type,
                });
            }
        }
    }

    entries
}

/// Encode the full document state as a yrs update.
pub fn encode_full_state(doc: &Doc) -> Vec<u8> {
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&StateVector::default())
}

/// Apply a remote yrs update to the manifest doc.
pub fn apply_update(doc: &Doc, update: &[u8]) {
    if let Ok(update) = Update::decode_v1(update) {
        let mut txn = doc.transact_mut();
        let _ = txn.apply_update(update);
    }
}

/// Add a file entry to the manifest.
pub fn add_entry(doc: &Doc, path: &str, parent: Option<&str>, size: u64, content_type: &str) {
    let map = doc.get_or_insert_map(MANIFEST_ROOT);
    let mut txn = doc.transact_mut();
    insert_entry(&map, &mut txn, path, parent, size, content_type);
}

/// Remove a file entry from the manifest.
pub fn remove_entry(doc: &Doc, path: &str) {
    let map = doc.get_or_insert_map(MANIFEST_ROOT);
    let mut txn = doc.transact_mut();
    map.remove(&mut txn, path);
}
