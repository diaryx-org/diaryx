//! Control messages from the sync server (JSON over WebSocket text frames).
//!
//! These messages are used by the siphonophore-based /sync2 protocol for
//! handshake, progress tracking, peer events, and session management.

use serde::Deserialize;

/// Control message from the sync server (JSON over WebSocket text frames).
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    /// Sync progress update from server.
    SyncProgress {
        /// Number of files completed.
        completed: usize,
        /// Total number of files to sync.
        total: usize,
    },
    /// Initial sync has completed.
    SyncComplete {
        /// Number of files synced.
        files_synced: usize,
    },
    /// A peer joined the sync session.
    PeerJoined {
        /// Number of peers currently connected.
        #[serde(default)]
        peer_count: usize,
    },
    /// A peer left the sync session.
    PeerLeft {
        /// Number of peers currently connected.
        #[serde(default)]
        peer_count: usize,
    },
    /// Focus list changed - files that any client is focused on.
    FocusListChanged {
        /// List of focused file paths.
        files: Vec<String>,
    },
    /// Files-Ready handshake: server sends file manifest before y-sync starts.
    FileManifest {
        /// List of files on the server.
        #[serde(default)]
        files: Vec<serde_json::Value>,
        /// Whether the client is new (no prior state).
        #[serde(default)]
        client_is_new: bool,
    },
    /// Files-Ready handshake: server sends CRDT state after client replies with FilesReady.
    CrdtState {
        /// Base64-encoded Y-CRDT state bytes.
        state: String,
    },
    /// Share session: guest joined confirmation.
    #[serde(alias = "session_joined")]
    SessionJoined {},
    /// Catch-all for other message types
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_progress() {
        let json = r#"{"type": "sync_progress", "completed": 5, "total": 10}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::SyncProgress { completed, total } => {
                assert_eq!(completed, 5);
                assert_eq!(total, 10);
            }
            _ => panic!("Expected SyncProgress"),
        }
    }

    #[test]
    fn test_sync_complete() {
        let json = r#"{"type": "sync_complete", "files_synced": 42}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::SyncComplete { files_synced } => assert_eq!(files_synced, 42),
            _ => panic!("Expected SyncComplete"),
        }
    }

    #[test]
    fn test_peer_joined() {
        let json = r#"{"type": "peer_joined", "peer_count": 3}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::PeerJoined { peer_count } => assert_eq!(peer_count, 3),
            _ => panic!("Expected PeerJoined"),
        }
    }

    #[test]
    fn test_peer_joined_default() {
        let json = r#"{"type": "peer_joined"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::PeerJoined { peer_count } => assert_eq!(peer_count, 0),
            _ => panic!("Expected PeerJoined"),
        }
    }

    #[test]
    fn test_peer_left() {
        let json = r#"{"type": "peer_left", "peer_count": 1}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::PeerLeft { peer_count } => assert_eq!(peer_count, 1),
            _ => panic!("Expected PeerLeft"),
        }
    }

    #[test]
    fn test_file_manifest() {
        let json =
            r#"{"type": "file_manifest", "files": [{"path": "a.md"}], "client_is_new": true}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::FileManifest {
                files,
                client_is_new,
            } => {
                assert_eq!(files.len(), 1);
                assert!(client_is_new);
            }
            _ => panic!("Expected FileManifest"),
        }
    }

    #[test]
    fn test_crdt_state() {
        let json = r#"{"type": "crdt_state", "state": "AQAAAA=="}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ControlMessage::CrdtState { state } => assert_eq!(state, "AQAAAA=="),
            _ => panic!("Expected CrdtState"),
        }
    }

    #[test]
    fn test_session_joined() {
        let json = r#"{"type": "session_joined"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ControlMessage::SessionJoined {}));
    }

    #[test]
    fn test_unknown_type() {
        let json = r#"{"type": "unknown_future_message", "data": "some value"}"#;
        let msg: ControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ControlMessage::Other));
    }

    #[test]
    fn test_invalid_json_fails() {
        let result: Result<ControlMessage, _> = serde_json::from_str("not valid json");
        assert!(result.is_err());
    }
}
