//! Binary envelope format for hot-path sync messages.
//!
//! Binary exports return an action list so the host can process multiple
//! outgoing messages/events from a single guest call without JSON overhead.
//!
//! ## Wire format
//!
//! ```text
//! [u16: num_actions]
//! for each action:
//!   [u8: action_type]   // 0=SendBinary, 1=SendText, 2=EmitEvent, 3=DownloadSnapshot
//!   [u32: payload_len]
//!   [payload_bytes]
//! ```

use diaryx_sync::SessionAction;

/// Action types in the binary envelope.
const ACTION_SEND_BINARY: u8 = 0;
const ACTION_SEND_TEXT: u8 = 1;
const ACTION_EMIT_EVENT: u8 = 2;
const ACTION_DOWNLOAD_SNAPSHOT: u8 = 3;

/// Encode a list of `SessionAction`s into the binary envelope format.
pub fn encode_actions(actions: &[SessionAction]) -> Vec<u8> {
    let num = actions.len() as u16;
    let mut buf = Vec::with_capacity(2 + actions.len() * 32);
    buf.extend_from_slice(&num.to_le_bytes());

    for action in actions {
        match action {
            SessionAction::SendBinary(data) => {
                buf.push(ACTION_SEND_BINARY);
                buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
                buf.extend_from_slice(data);
            }
            SessionAction::SendText(text) => {
                let bytes = text.as_bytes();
                buf.push(ACTION_SEND_TEXT);
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
            SessionAction::Emit(event) => {
                let json = serde_json::to_vec(event).unwrap_or_default();
                buf.push(ACTION_EMIT_EVENT);
                buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
                buf.extend_from_slice(&json);
            }
            SessionAction::DownloadSnapshot { workspace_id } => {
                let bytes = workspace_id.as_bytes();
                buf.push(ACTION_DOWNLOAD_SNAPSHOT);
                buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(bytes);
            }
        }
    }

    buf
}

/// Decoded action from the binary envelope.
///
/// EmitEvent carries raw JSON bytes since `SyncEvent` only derives `Serialize`.
/// The host side is responsible for deserializing the event JSON.
#[derive(Debug)]
pub enum DecodedAction {
    SendBinary(Vec<u8>),
    SendText(String),
    /// Raw JSON bytes of a serialized `SyncEvent`.
    EmitEvent(Vec<u8>),
    DownloadSnapshot(String),
}

/// Decode a binary envelope into a list of actions.
pub fn decode_actions(data: &[u8]) -> Result<Vec<DecodedAction>, String> {
    if data.len() < 2 {
        return Err("Buffer too short for action count".into());
    }

    let num = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut offset = 2;
    let mut actions = Vec::with_capacity(num);

    for _ in 0..num {
        if offset >= data.len() {
            return Err("Unexpected end of buffer".into());
        }

        let action_type = data[offset];
        offset += 1;

        if offset + 4 > data.len() {
            return Err("Unexpected end of buffer reading payload length".into());
        }
        let payload_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if offset + payload_len > data.len() {
            return Err("Payload exceeds buffer".into());
        }
        let payload = &data[offset..offset + payload_len];
        offset += payload_len;

        let action = match action_type {
            ACTION_SEND_BINARY => DecodedAction::SendBinary(payload.to_vec()),
            ACTION_SEND_TEXT => {
                let text = String::from_utf8(payload.to_vec())
                    .map_err(|e| format!("Invalid UTF-8 in SendText: {e}"))?;
                DecodedAction::SendText(text)
            }
            ACTION_EMIT_EVENT => DecodedAction::EmitEvent(payload.to_vec()),
            ACTION_DOWNLOAD_SNAPSHOT => {
                let id = String::from_utf8(payload.to_vec())
                    .map_err(|e| format!("Invalid UTF-8 in DownloadSnapshot: {e}"))?;
                DecodedAction::DownloadSnapshot(id)
            }
            other => return Err(format!("Unknown action type: {other}")),
        };

        actions.push(action);
    }

    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_empty() {
        let encoded = encode_actions(&[]);
        let decoded = decode_actions(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn roundtrip_send_binary() {
        let actions = vec![SessionAction::SendBinary(vec![1, 2, 3, 4])];
        let encoded = encode_actions(&actions);
        let decoded = decode_actions(&encoded).unwrap();
        assert_eq!(decoded.len(), 1);
        match &decoded[0] {
            DecodedAction::SendBinary(data) => assert_eq!(data, &[1, 2, 3, 4]),
            other => panic!("Expected SendBinary, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_mixed() {
        let actions = vec![
            SessionAction::SendBinary(vec![0xAB]),
            SessionAction::SendText("hello".into()),
            SessionAction::DownloadSnapshot {
                workspace_id: "ws-123".into(),
            },
        ];
        let encoded = encode_actions(&actions);
        let decoded = decode_actions(&encoded).unwrap();
        assert_eq!(decoded.len(), 3);
    }
}
