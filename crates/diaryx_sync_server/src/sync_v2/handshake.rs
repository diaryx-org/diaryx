//! Files-Ready handshake layer for siphonophore.
//!
//! This module implements the Files-Ready handshake protocol that prevents
//! tombstoning when a client reconnects. The handshake ensures clients have
//! downloaded all files before CRDT sync begins.
//!
//! ## Protocol Flow
//!
//! 1. Client connects to WebSocket
//! 2. Server sends `FileManifest` (JSON text message)
//! 3. Client downloads files via separate API calls
//! 4. Client sends `FilesReady` (JSON text message)
//! 5. Server sends `CrdtState` (JSON text message with base64 state)
//! 6. Normal siphonophore y-sync begins
//!
//! ## Message Format
//!
//! Control messages are JSON text messages:
//!
//! ```json
//! {"type": "file_manifest", "files": [...], "client_is_new": true}
//! {"type": "files_ready"}
//! {"type": "crdt_state", "state": "<base64>"}
//! {"type": "focus", "files": ["path/to/file.md"]}
//! {"type": "unfocus", "files": ["path/to/file.md"]}
//! {"type": "peer_joined", "guest_id": "...", "peer_count": 2}
//! {"type": "peer_left", "guest_id": "...", "peer_count": 1}
//! {"type": "session_ended"}
//! ```

use axum::extract::ws::{Message, WebSocket};
use diaryx_core::crdt::{CrdtStorage, SqliteStorage};
use futures::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info};

// Re-export shared protocol types for backward compatibility
use diaryx_sync::protocol::AuthenticatedUser;
pub use diaryx_sync::protocol::{
    ClientControlMessage, HandshakeState, ManifestFileEntry, ServerControlMessage,
};

/// Connection context for tracking state.
pub struct ConnectionContext {
    pub user: AuthenticatedUser,
    pub workspace_id: String,
    pub handshake_state: HandshakeState,
    pub focused_files: HashSet<String>,
    pub storage: Arc<SqliteStorage>,
}

impl ConnectionContext {
    pub fn new(user: AuthenticatedUser, storage: Arc<SqliteStorage>) -> Self {
        let workspace_id = user.workspace_id.clone();
        Self {
            user,
            workspace_id,
            handshake_state: HandshakeState::AwaitingManifest,
            focused_files: HashSet::new(),
            storage,
        }
    }

    /// Generate file manifest from workspace CRDT.
    pub fn generate_file_manifest(&self) -> Result<Vec<ManifestFileEntry>, String> {
        // Query active files from storage
        let files = self
            .storage
            .query_active_files()
            .map_err(|e| format!("Failed to query files: {}", e))?;

        Ok(files
            .into_iter()
            .map(|(path, title, part_of)| ManifestFileEntry {
                doc_id: format!("body:{}/{}", self.workspace_id, path),
                filename: path,
                title,
                part_of,
                deleted: false,
            })
            .collect())
    }

    /// Get full CRDT state for the workspace.
    pub fn get_workspace_state(&self) -> Result<Vec<u8>, String> {
        let storage_key = format!("workspace:{}", self.workspace_id);
        self.storage
            .load_doc(&storage_key)
            .map_err(|e| format!("Failed to load workspace state: {}", e))?
            .ok_or_else(|| "No workspace state found".to_string())
    }
}

/// Handle the Files-Ready handshake for a new connection.
///
/// This function implements the handshake protocol before delegating to
/// siphonophore for y-sync.
///
/// Returns the WebSocket split for siphonophore to use after handshake.
pub async fn perform_handshake(
    socket: WebSocket,
    ctx: &mut ConnectionContext,
) -> Result<(SplitSink<WebSocket, Message>, SplitStream<WebSocket>), String> {
    let (mut sink, mut stream) = socket.split();

    // Step 1: Generate and send file manifest
    let manifest = ctx.generate_file_manifest()?;
    let client_is_new = manifest.is_empty(); // New workspace has no files

    let manifest_msg = ServerControlMessage::FileManifest {
        files: manifest,
        client_is_new,
    };

    let json = serde_json::to_string(&manifest_msg)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;

    sink.send(Message::Text(json.into()))
        .await
        .map_err(|e| format!("Failed to send manifest: {}", e))?;

    ctx.handshake_state = HandshakeState::AwaitingFilesReady;
    debug!("Sent file manifest to client");

    // Step 2: Wait for FilesReady from client
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<ClientControlMessage>(&text) {
                    Ok(ClientControlMessage::FilesReady) => {
                        debug!("Received FilesReady from client");
                        break;
                    }
                    Ok(other) => {
                        debug!("Ignoring control message during handshake: {:?}", other);
                    }
                    Err(e) => {
                        debug!("Ignoring non-control text message: {}", e);
                    }
                }
            }
            Some(Ok(Message::Close(_))) => {
                return Err("Client closed connection during handshake".to_string());
            }
            Some(Err(e)) => {
                return Err(format!("WebSocket error during handshake: {}", e));
            }
            None => {
                return Err("Connection closed during handshake".to_string());
            }
            _ => {
                // Ignore binary messages during handshake
            }
        }
    }

    // Step 3: Send CRDT state
    match ctx.get_workspace_state() {
        Ok(state) => {
            let state_b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &state);
            let state_msg = ServerControlMessage::CrdtState { state: state_b64 };
            let json = serde_json::to_string(&state_msg)
                .map_err(|e| format!("Failed to serialize state: {}", e))?;

            sink.send(Message::Text(json.into()))
                .await
                .map_err(|e| format!("Failed to send state: {}", e))?;

            debug!("Sent CRDT state to client ({} bytes)", state.len());
        }
        Err(e) => {
            // No state yet, which is fine for new workspaces
            debug!("No CRDT state to send: {}", e);
        }
    }

    ctx.handshake_state = HandshakeState::Complete;
    info!("Handshake complete for workspace {}", ctx.workspace_id);

    Ok((sink, stream))
}

/// Handle control messages during active sync.
///
/// This processes focus/unfocus and other control messages that aren't
/// part of the y-sync protocol.
pub async fn handle_control_message(
    text: &str,
    ctx: &mut ConnectionContext,
    _broadcast_tx: &broadcast::Sender<ServerControlMessage>,
) -> Option<ServerControlMessage> {
    match serde_json::from_str::<ClientControlMessage>(text) {
        Ok(ClientControlMessage::Focus { files }) => {
            for file in &files {
                ctx.focused_files.insert(file.clone());
            }
            debug!("Client focused on {} files", files.len());
            // Could broadcast FocusListChanged here
            None
        }
        Ok(ClientControlMessage::Unfocus { files }) => {
            for file in &files {
                ctx.focused_files.remove(file);
            }
            debug!("Client unfocused {} files", files.len());
            None
        }
        Ok(ClientControlMessage::FilesReady) => {
            debug!("Ignoring FilesReady after handshake");
            None
        }
        Err(_) => {
            // Not a control message, might be siphonophore control message
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_control_message_serialization() {
        let msg = ServerControlMessage::FileManifest {
            files: vec![ManifestFileEntry {
                doc_id: "body:ws1/test.md".to_string(),
                filename: "test.md".to_string(),
                title: Some("Test".to_string()),
                part_of: None,
                deleted: false,
            }],
            client_is_new: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("file_manifest"));
        assert!(json.contains("test.md"));
    }

    #[test]
    fn test_client_control_message_deserialization() {
        let json = r#"{"type": "files_ready"}"#;
        let msg: ClientControlMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, ClientControlMessage::FilesReady));

        let json = r#"{"type": "focus", "files": ["a.md", "b.md"]}"#;
        let msg: ClientControlMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientControlMessage::Focus { files } => {
                assert_eq!(files.len(), 2);
            }
            _ => panic!("Expected Focus message"),
        }
    }
}
