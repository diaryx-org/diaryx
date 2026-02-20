//! Tokio-tungstenite WebSocket transport implementation.
//!
//! Wraps `tokio_tungstenite::connect_async()` and implements the `SyncTransport` trait.

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::transport::{SyncTransport, TransportConnector, TransportError, WsMessage};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A `SyncTransport` backed by tokio-tungstenite.
pub struct TokioTransport {
    ws: WsStream,
}

impl TokioTransport {
    /// Connect to a WebSocket URL and return a `TokioTransport`.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (ws, _response) = connect_async(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(Self { ws })
    }
}

#[async_trait::async_trait]
impl SyncTransport for TokioTransport {
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), TransportError> {
        self.ws
            .send(Message::Binary(data.into()))
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn send_text(&mut self, text: String) -> Result<(), TransportError> {
        self.ws
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn send_ping(&mut self) -> Result<(), TransportError> {
        self.ws
            .send(Message::Ping(vec![].into()))
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn recv(&mut self) -> Option<Result<WsMessage, TransportError>> {
        match self.ws.next().await {
            Some(Ok(Message::Binary(data))) => Some(Ok(WsMessage::Binary(data.to_vec()))),
            Some(Ok(Message::Text(text))) => Some(Ok(WsMessage::Text(text.to_string()))),
            Some(Ok(Message::Ping(data))) => Some(Ok(WsMessage::Ping(data.to_vec()))),
            Some(Ok(Message::Pong(data))) => Some(Ok(WsMessage::Pong(data.to_vec()))),
            Some(Ok(Message::Close(_))) => Some(Ok(WsMessage::Close)),
            Some(Ok(Message::Frame(_))) => {
                // Raw frames are not expected; skip.
                Some(Ok(WsMessage::Pong(vec![])))
            }
            Some(Err(e)) => Some(Err(TransportError::Other(e.to_string()))),
            None => None,
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        self.ws
            .close(None)
            .await
            .map_err(|e| TransportError::Other(e.to_string()))
    }
}

/// Connector that creates `TokioTransport` connections via `tokio-tungstenite`.
pub struct TokioConnector;

#[async_trait::async_trait]
impl TransportConnector for TokioConnector {
    type Transport = TokioTransport;

    async fn connect(&self, url: &str) -> Result<Self::Transport, TransportError> {
        TokioTransport::connect(url).await
    }
}
