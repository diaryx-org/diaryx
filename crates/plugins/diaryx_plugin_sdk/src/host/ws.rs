//! WebSocket bridge for real-time communication.
//!
//! Requires the `ws` feature. The host manages the actual WebSocket connection;
//! the plugin sends structured requests through this bridge.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;

use super::*;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsRequest<'a> {
    Connect {
        server_url: &'a str,
        workspace_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_token: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_code: Option<&'a str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        write_to_disk: Option<bool>,
    },
    SendBinary {
        data: String,
    },
    SendText {
        text: &'a str,
    },
    Disconnect,
}

fn ws_request_json(request: &WsRequest<'_>) -> Result<(), String> {
    let payload = serde_json::to_string(request)
        .map_err(|e| format!("Failed to serialize websocket request: {e}"))?;
    raw_request(&payload)?;
    Ok(())
}

/// Send a raw JSON request through the WebSocket bridge.
pub fn raw_request(payload: &str) -> Result<String, String> {
    unsafe { host_ws_request(payload.to_string()) }
        .map_err(|e| format!("host_ws_request failed: {e}"))
}

/// Connect to a WebSocket server.
pub fn connect(
    server_url: &str,
    workspace_id: &str,
    auth_token: Option<&str>,
    session_code: Option<&str>,
    write_to_disk: Option<bool>,
) -> Result<(), String> {
    ws_request_json(&WsRequest::Connect {
        server_url,
        workspace_id,
        auth_token,
        session_code,
        write_to_disk,
    })
}

/// Send binary data over the WebSocket.
pub fn send_binary(data: &[u8]) -> Result<(), String> {
    ws_request_json(&WsRequest::SendBinary {
        data: BASE64.encode(data),
    })
}

/// Send a text message over the WebSocket.
pub fn send_text(text: &str) -> Result<(), String> {
    ws_request_json(&WsRequest::SendText { text })
}

/// Disconnect from the WebSocket server.
pub fn disconnect() -> Result<(), String> {
    ws_request_json(&WsRequest::Disconnect)
}
