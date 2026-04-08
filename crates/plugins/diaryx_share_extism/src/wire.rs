//! Wire protocol helpers for share session WebSocket communication.
//!
//! Binary framing uses u16 LE doc_id length to support long file paths.
//! Text messages carry JSON control messages for session coordination.

use serde::{Deserialize, Serialize};

/// Frame a binary message with doc_id prefix.
///
/// Format: `[u16 LE: doc_id_len][doc_id bytes][payload bytes]`
pub fn frame_binary(doc_id: &str, payload: &[u8]) -> Vec<u8> {
    let doc_id_bytes = doc_id.as_bytes();
    let len = doc_id_bytes.len() as u16;
    let mut frame = Vec::with_capacity(2 + doc_id_bytes.len() + payload.len());
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(doc_id_bytes);
    frame.extend_from_slice(payload);
    frame
}

/// Parse a binary message into (doc_id, payload).
pub fn unframe_binary(data: &[u8]) -> Option<(&str, &[u8])> {
    if data.len() < 2 {
        return None;
    }
    let doc_id_len = u16::from_le_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + doc_id_len {
        return None;
    }
    let doc_id = std::str::from_utf8(&data[2..2 + doc_id_len]).ok()?;
    let payload = &data[2 + doc_id_len..];
    Some((doc_id, payload))
}

/// Control messages received from the server or peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    FileRequested { path: String, requester_id: String },
    FileReady { path: String },
    PeerJoined { guest_id: String, peer_count: usize },
    PeerLeft { guest_id: String, peer_count: usize },
    SessionEnded,
}

pub fn parse_control_message(text: &str) -> Option<ControlMessage> {
    serde_json::from_str(text).ok()
}

pub fn make_file_request(path: &str) -> String {
    serde_json::json!({
        "type": "file_request",
        "path": path,
    })
    .to_string()
}

pub fn make_file_ready(path: &str) -> String {
    serde_json::json!({
        "type": "file_ready",
        "path": path,
    })
    .to_string()
}

pub fn make_session_end() -> String {
    serde_json::json!({
        "type": "session_end",
    })
    .to_string()
}

/// Build the manifest doc_id for a namespace.
pub fn manifest_doc_id(namespace_id: &str) -> String {
    format!("manifest:{}", namespace_id)
}

/// Build the file doc_id for a namespace + path.
pub fn file_doc_id(namespace_id: &str, path: &str) -> String {
    format!("file:{}/{}", namespace_id, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let doc_id = "file:ns-123/notes/hello.md";
        let payload = b"some yrs update bytes";
        let framed = frame_binary(doc_id, payload);
        let (parsed_id, parsed_payload) = unframe_binary(&framed).unwrap();
        assert_eq!(parsed_id, doc_id);
        assert_eq!(parsed_payload, payload);
    }

    #[test]
    fn frame_empty_payload() {
        let framed = frame_binary("manifest:abc", b"");
        let (id, payload) = unframe_binary(&framed).unwrap();
        assert_eq!(id, "manifest:abc");
        assert!(payload.is_empty());
    }

    #[test]
    fn unframe_too_short() {
        assert!(unframe_binary(&[]).is_none());
        assert!(unframe_binary(&[5, 0]).is_none()); // claims 5 bytes but none follow
    }

    #[test]
    fn control_message_roundtrip() {
        let msg = make_file_request("notes/foo.md");
        let parsed = parse_control_message(&msg).unwrap();
        match parsed {
            ControlMessage::FileRequested { .. } => {
                // file_request from client is different from file_requested from server
                panic!("should not parse as FileRequested");
            }
            _ => {}
        }
        // file_request is a client message, not in ControlMessage enum
        // It parses as None since our enum only has server messages
    }

    #[test]
    fn parse_file_requested() {
        let json = r#"{"type":"file_requested","path":"notes/foo.md","requester_id":"user-1"}"#;
        let parsed = parse_control_message(json).unwrap();
        match parsed {
            ControlMessage::FileRequested {
                ref path,
                ref requester_id,
            } => {
                assert_eq!(path, "notes/foo.md");
                assert_eq!(requester_id, "user-1");
            }
            _ => panic!("wrong variant"),
        }
    }
}
