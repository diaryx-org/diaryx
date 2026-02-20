//! WebSocket transport trait for sync client.
//!
//! Provides an abstraction over the WebSocket connection used by `SyncClient`,
//! allowing different backends (tokio-tungstenite for native, or mock for tests).

use std::fmt;

/// A WebSocket message received from or sent to the server.
#[derive(Debug, Clone)]
pub enum WsMessage {
    /// Binary data (Y-sync protocol messages).
    Binary(Vec<u8>),
    /// Text data (JSON control messages).
    Text(String),
    /// Ping frame (keepalive).
    Ping(Vec<u8>),
    /// Pong frame (keepalive response).
    Pong(Vec<u8>),
    /// Connection close frame.
    Close,
}

/// Error type for transport operations.
#[derive(Debug)]
pub enum TransportError {
    /// Failed to establish a connection.
    ConnectionFailed(String),
    /// Failed to send a message.
    SendFailed(String),
    /// The connection was closed.
    Closed,
    /// Any other error.
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::ConnectionFailed(msg) => write!(f, "Connection failed: {}", msg),
            TransportError::SendFailed(msg) => write!(f, "Send failed: {}", msg),
            TransportError::Closed => write!(f, "Connection closed"),
            TransportError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for TransportError {}

/// Factory for creating transport connections.
///
/// Separates connection establishment from the transport's send/recv lifecycle,
/// allowing `SyncClient` to be generic over the transport implementation.
#[async_trait::async_trait]
pub trait TransportConnector: Send + Sync {
    /// The transport type produced by this connector.
    type Transport: SyncTransport;

    /// Establish a new WebSocket connection to the given URL.
    async fn connect(&self, url: &str) -> Result<Self::Transport, TransportError>;
}

/// Async WebSocket transport trait.
///
/// Implementations provide the actual WebSocket connectivity. The `SyncClient`
/// uses this trait rather than directly depending on tokio-tungstenite, enabling
/// testability and future alternative transports.
#[async_trait::async_trait]
pub trait SyncTransport: Send {
    /// Send a binary message.
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), TransportError>;

    /// Send a text message.
    async fn send_text(&mut self, text: String) -> Result<(), TransportError>;

    /// Send a ping frame.
    async fn send_ping(&mut self) -> Result<(), TransportError>;

    /// Receive the next message, or `None` if the connection is closed.
    async fn recv(&mut self) -> Option<Result<WsMessage, TransportError>>;

    /// Close the connection gracefully.
    async fn close(&mut self) -> Result<(), TransportError>;
}
