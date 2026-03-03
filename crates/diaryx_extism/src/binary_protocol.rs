//! Binary action envelope parsing for the host side.
//!
//! Decodes the binary envelope format returned by guest binary exports.
//! See `diaryx_sync_extism::binary_protocol` for the encoding side.
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

/// Action types in the binary envelope.
const ACTION_SEND_BINARY: u8 = 0;
const ACTION_SEND_TEXT: u8 = 1;
const ACTION_EMIT_EVENT: u8 = 2;
const ACTION_DOWNLOAD_SNAPSHOT: u8 = 3;

/// Decoded action from the binary envelope.
#[derive(Debug)]
pub enum DecodedAction {
    /// Send binary data over the WebSocket.
    SendBinary(Vec<u8>),
    /// Send text data over the WebSocket.
    SendText(String),
    /// Emit a sync event (raw JSON bytes).
    EmitEvent(Vec<u8>),
    /// Download a workspace snapshot by ID.
    DownloadSnapshot(String),
}

/// Decode a binary action envelope into a list of actions.
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
    fn decode_empty() {
        let data = [0u8, 0]; // num_actions = 0
        let actions = decode_actions(&data).unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn decode_send_binary() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes()); // 1 action
        data.push(ACTION_SEND_BINARY);
        data.extend_from_slice(&4u32.to_le_bytes()); // 4 bytes
        data.extend_from_slice(&[1, 2, 3, 4]);

        let actions = decode_actions(&data).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            DecodedAction::SendBinary(bytes) => assert_eq!(bytes, &[1, 2, 3, 4]),
            other => panic!("Expected SendBinary, got {other:?}"),
        }
    }

    #[test]
    fn decode_mixed() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_le_bytes()); // 2 actions

        // SendText "hi"
        data.push(ACTION_SEND_TEXT);
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(b"hi");

        // DownloadSnapshot "ws-1"
        data.push(ACTION_DOWNLOAD_SNAPSHOT);
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"ws-1");

        let actions = decode_actions(&data).unwrap();
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            DecodedAction::SendText(t) => assert_eq!(t, "hi"),
            other => panic!("Expected SendText, got {other:?}"),
        }
        match &actions[1] {
            DecodedAction::DownloadSnapshot(id) => assert_eq!(id, "ws-1"),
            other => panic!("Expected DownloadSnapshot, got {other:?}"),
        }
    }

    #[test]
    fn decode_truncated() {
        let data = [1u8, 0]; // says 1 action but no action data
        assert!(decode_actions(&data).is_err());
    }
}
