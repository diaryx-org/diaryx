//! Type conversions between `diaryx_sync` history types and `diaryx_core::crdt` history types.
//!
//! These are needed by [`SyncPlugin::handle_typed_command`] which must return
//! `diaryx_core::command::Response` values containing `diaryx_core::crdt` types,
//! while the plugin internally uses `diaryx_sync::history` types.

use crate::history;

/// Convert sync `HistoryEntry` → core `HistoryEntry`.
impl From<history::HistoryEntry> for diaryx_core::crdt::HistoryEntry {
    fn from(e: history::HistoryEntry) -> Self {
        Self {
            update_id: e.update_id,
            timestamp: e.timestamp,
            origin: e.origin,
            files_changed: e.files_changed,
            device_id: e.device_id,
            device_name: e.device_name,
        }
    }
}

/// Convert sync `ChangeType` → core `ChangeType`.
impl From<history::ChangeType> for diaryx_core::crdt::ChangeType {
    fn from(c: history::ChangeType) -> Self {
        match c {
            history::ChangeType::Added => Self::Added,
            history::ChangeType::Modified => Self::Modified,
            history::ChangeType::Deleted => Self::Deleted,
            history::ChangeType::Restored => Self::Restored,
        }
    }
}

/// Convert sync `FileDiff` → core `FileDiff`.
///
/// `FileMetadata` is the same type in both crates (re-exported from core),
/// so `old_metadata` and `new_metadata` don't need conversion.
impl From<history::FileDiff> for diaryx_core::crdt::FileDiff {
    fn from(d: history::FileDiff) -> Self {
        Self {
            path: d.path,
            change_type: d.change_type.into(),
            old_metadata: d.old_metadata,
            new_metadata: d.new_metadata,
        }
    }
}
