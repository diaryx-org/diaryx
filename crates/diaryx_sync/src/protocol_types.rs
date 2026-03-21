//! Platform-agnostic sync protocol types.
//!
//! These types define the sync handshake and control message protocol.
//! They are intentionally free of runtime dependencies (no tokio, no siphonophore)
//! so they can be used by both the native siphonophore-based server and the
//! Cloudflare Workers Durable Object adapter.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use yrs::{Update, updates::decoder::Decode};

// ==================== Document Types ====================

/// Document type determined from doc_id prefix.
#[derive(Debug, Clone, PartialEq)]
pub enum DocType {
    /// Workspace metadata CRDT (workspace:<id>)
    Workspace(String),
    /// Body document CRDT (body:<workspace_id>/<body_id>)
    Body {
        workspace_id: String,
        body_id: String,
    },
    /// Share session manifest CRDT (manifest:<namespace_id>)
    Manifest(String),
    /// Share session file CRDT (file:<namespace_id>/<path>)
    File { namespace_id: String, path: String },
}

impl DocType {
    /// Parse a doc_id into a DocType.
    pub fn parse(doc_id: &str) -> Option<Self> {
        if let Some(workspace_id) = doc_id.strip_prefix("workspace:") {
            Some(DocType::Workspace(workspace_id.to_string()))
        } else if let Some(rest) = doc_id.strip_prefix("body:") {
            let (workspace_id, body_id) = rest.split_once('/')?;
            Some(DocType::Body {
                workspace_id: workspace_id.to_string(),
                body_id: body_id.to_string(),
            })
        } else if let Some(namespace_id) = doc_id.strip_prefix("manifest:") {
            Some(DocType::Manifest(namespace_id.to_string()))
        } else if let Some(rest) = doc_id.strip_prefix("file:") {
            let (namespace_id, path) = rest.split_once('/')?;
            Some(DocType::File {
                namespace_id: namespace_id.to_string(),
                path: path.to_string(),
            })
        } else {
            // Legacy format: just workspace_id (treat as workspace doc)
            Some(DocType::Workspace(doc_id.to_string()))
        }
    }

    /// Get the workspace_id for this document.
    pub fn workspace_id(&self) -> &str {
        match self {
            DocType::Workspace(id) => id,
            DocType::Body { workspace_id, .. } => workspace_id,
            DocType::Manifest(id) => id,
            DocType::File { namespace_id, .. } => namespace_id,
        }
    }

    /// Get the storage key for this document.
    pub fn storage_key(&self) -> String {
        match self {
            DocType::Workspace(id) => format!("workspace:{}", id),
            DocType::Body {
                workspace_id,
                body_id,
            } => format!("body:{}/{}", workspace_id, body_id),
            DocType::Manifest(id) => format!("manifest:{}", id),
            DocType::File { namespace_id, path } => format!("file:{}/{}", namespace_id, path),
        }
    }
}

// ==================== Handshake Protocol Types ====================

/// File entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    pub doc_id: String,
    pub filename: String,
    pub title: Option<String>,
    pub part_of: Option<String>,
    pub deleted: bool,
}

/// Control messages sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerControlMessage {
    /// File manifest for initial sync.
    FileManifest {
        files: Vec<ManifestFileEntry>,
        client_is_new: bool,
    },
    /// Full CRDT state after files are ready.
    CrdtState {
        state: String, // Base64 encoded
    },
    /// Peer joined the session.
    PeerJoined { guest_id: String, peer_count: usize },
    /// Peer left the session.
    PeerLeft { guest_id: String, peer_count: usize },
    /// Session has ended.
    SessionEnded,
    /// Sync progress update.
    SyncProgress { completed: usize, total: usize },
    /// Initial sync complete.
    SyncComplete { files_synced: usize },
    /// Focus list changed.
    FocusListChanged { files: Vec<String> },
    /// File content requested by a peer (relayed by server with requester identity).
    FileRequested { path: String, requester_id: String },
}

/// Control messages sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientControlMessage {
    /// Client is ready with all files downloaded.
    FilesReady,
    /// Client wants to focus on files.
    Focus { files: Vec<String> },
    /// Client wants to unfocus files.
    Unfocus { files: Vec<String> },
    /// Client requests a file's content be loaded into a CRDT doc.
    FileRequest { path: String },
    /// Client signals it has pushed initial content for a file doc.
    FileReady { path: String },
    /// Host signals the share session is ending.
    SessionEnd,
}

/// State for a client connection during handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeState {
    /// Awaiting file manifest (server needs to send it)
    AwaitingManifest,
    /// Manifest sent, awaiting FilesReady from client
    AwaitingFilesReady,
    /// Handshake complete, normal sync in progress
    Complete,
}

/// User information stored in the connection context after authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub workspace_id: String,
    pub device_id: Option<String>,
    pub is_guest: bool,
    pub read_only: bool,
}

// ==================== Wire Format Utilities ====================

/// Strip Y-sync protocol framing from a message, returning just the Y-CRDT update bytes.
///
/// Y-sync messages have the wire format: `[MSG_SYNC varint][subtype varint][lib0 buf]`
/// where MSG_SYNC=0, subtype is SyncStep2=1 or SyncUpdate=2, and the lib0 buf is
/// `[varint length][update bytes]`. Since the varint values 0, 1, 2 each encode as a
/// single byte, we can check the first two bytes and then decode the length varint.
///
/// Returns `None` if the data does not look like a framed Y-sync message, in which
/// case the caller should treat the data as a raw Y-CRDT update.
pub fn strip_ysync_framing(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 3 || data[0] != 0 || !matches!(data[1], 1 | 2) {
        return None;
    }
    let mut pos = 2usize;
    let mut len: usize = 0;
    let mut shift = 0u32;
    loop {
        if pos >= data.len() {
            return None;
        }
        let byte = data[pos];
        pos += 1;
        len |= ((byte & 0x7F) as usize) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
    if pos + len > data.len() {
        return None;
    }
    Some(data[pos..pos + len].to_vec())
}

/// Choose the safest payload to persist as a Y update.
///
/// Priority:
/// 1) Persist raw bytes when they already decode as a Y update.
/// 2) If raw bytes do not decode, try stripping Y-sync framing and decode again.
/// 3) If neither decodes, persist raw bytes (for observability/forensics).
pub fn select_persistable_update(data: &[u8]) -> (Cow<'_, [u8]>, &'static str) {
    if Update::decode_v1(data).is_ok() {
        return (Cow::Borrowed(data), "raw");
    }

    if let Some(stripped) = strip_ysync_framing(data)
        && Update::decode_v1(&stripped).is_ok()
    {
        return (Cow::Owned(stripped), "stripped");
    }

    (Cow::Borrowed(data), "raw_undecodable")
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_type_parse_workspace() {
        let dt = DocType::parse("workspace:abc123").unwrap();
        assert_eq!(dt, DocType::Workspace("abc123".to_string()));
        assert_eq!(dt.workspace_id(), "abc123");
        assert_eq!(dt.storage_key(), "workspace:abc123");
    }

    #[test]
    fn test_doc_type_parse_body() {
        let dt = DocType::parse("body:abc123/some-uuid-123").unwrap();
        assert_eq!(
            dt,
            DocType::Body {
                workspace_id: "abc123".to_string(),
                body_id: "some-uuid-123".to_string(),
            }
        );
        assert_eq!(dt.workspace_id(), "abc123");
        assert_eq!(dt.storage_key(), "body:abc123/some-uuid-123");
    }

    #[test]
    fn test_doc_type_parse_legacy() {
        let dt = DocType::parse("abc123").unwrap();
        assert_eq!(dt, DocType::Workspace("abc123".to_string()));
    }

    #[test]
    fn test_doc_type_parse_manifest() {
        let dt = DocType::parse("manifest:ns-uuid-123").unwrap();
        assert_eq!(dt, DocType::Manifest("ns-uuid-123".to_string()));
        assert_eq!(dt.workspace_id(), "ns-uuid-123");
        assert_eq!(dt.storage_key(), "manifest:ns-uuid-123");
    }

    #[test]
    fn test_doc_type_parse_file() {
        let dt = DocType::parse("file:ns-uuid-123/notes/foo.md").unwrap();
        assert_eq!(
            dt,
            DocType::File {
                namespace_id: "ns-uuid-123".to_string(),
                path: "notes/foo.md".to_string(),
            }
        );
        assert_eq!(dt.workspace_id(), "ns-uuid-123");
        assert_eq!(dt.storage_key(), "file:ns-uuid-123/notes/foo.md");
    }
}
