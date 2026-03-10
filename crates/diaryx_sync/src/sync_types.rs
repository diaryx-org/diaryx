//! Shared sync types available on all platforms (native + WASM).
//!
//! These types are NOT feature-gated, so they can be used by both
//! `SyncClient` (native) and `WasmSyncClient` (WASM).

/// Configuration for a sync session.
///
/// Platform-agnostic config shared by native `SyncClient` and WASM `WasmSyncClient`.
#[derive(Debug, Clone)]
pub struct SyncSessionConfig {
    /// Workspace ID to sync.
    pub workspace_id: String,
    /// Whether to write changes to disk (false for one-shot / guest mode).
    pub write_to_disk: bool,
}

/// Events emitted by the sync session to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SyncEvent {
    /// Sync status changed.
    StatusChanged {
        /// The new status.
        status: SyncStatus,
    },
    /// Sync progress update.
    Progress {
        /// Number of files completed.
        completed: usize,
        /// Total number of files.
        total: usize,
    },
    /// Workspace files changed (metadata sync).
    FilesChanged {
        /// Changed file paths.
        files: Vec<String>,
    },
    /// A body document changed.
    BodyChanged {
        /// Path of the changed file.
        file_path: String,
        /// Latest body content after applying the remote update.
        body: String,
    },
    /// An error occurred.
    Error {
        /// Error message.
        message: String,
    },
    /// A peer joined the sync session.
    PeerJoined {
        /// Current peer count.
        peer_count: usize,
    },
    /// A peer left the sync session.
    PeerLeft {
        /// Current peer count.
        peer_count: usize,
    },
    /// Initial sync completed with file count.
    SyncComplete {
        /// Number of files synced.
        files_synced: usize,
    },
    /// The server's focus list changed.
    FocusListChanged {
        /// Currently focused file paths.
        files: Vec<String>,
    },
}

/// Current sync status.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SyncStatus {
    /// Connecting to the server.
    Connecting,
    /// Connected to the server.
    Connected,
    /// Performing initial sync.
    Syncing,
    /// Initial sync complete, watching for changes.
    Synced,
    /// Reconnecting after disconnect.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
    },
    /// Disconnected from the server.
    Disconnected,
}
