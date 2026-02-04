//! Tokio-based WebSocket transport for native platforms.
//!
//! This module provides `TokioTransport`, an implementation of `SyncTransport`
//! that uses tokio-tungstenite for WebSocket connections on native platforms
//! (CLI, Tauri, etc.).
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{TokioTransport, SyncConfig, SyncTransport};
//!
//! let transport = TokioTransport::new();
//! let config = SyncConfig::metadata("wss://sync.example.com".to_string(), "workspace".to_string());
//!
//! transport.connect(&config).await?;
//! transport.send(b"sync message").await?;
//! transport.disconnect().await?;
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::transport::{
    ConnectionStatus, MessageCallback, StatusCallback, SyncConfig, SyncTransport,
};
use crate::error::{DiaryxError, Result};

/// Outgoing WebSocket message type.
#[derive(Debug, Clone)]
enum OutgoingMessage {
    Binary(Vec<u8>),
    Text(String),
}

/// Sender half for outgoing WebSocket messages.
type WsSender = mpsc::UnboundedSender<OutgoingMessage>;

/// Default no-op message callback.
fn default_message_callback(_: &[u8]) -> Option<Vec<u8>> {
    None
}

/// Tokio-based WebSocket transport.
///
/// Uses tokio-tungstenite for native WebSocket connections with async I/O.
/// This transport is designed for CLI and native desktop applications.
///
/// # Thread Safety
///
/// `TokioTransport` is `Send + Sync` and can be shared across async tasks.
/// The internal state is protected by `RwLock` and atomic operations.
pub struct TokioTransport {
    /// Sender for outgoing messages (to the WebSocket task).
    sender: RwLock<Option<WsSender>>,

    /// Connection status.
    status: RwLock<ConnectionStatus>,

    /// Whether currently connected.
    connected: AtomicBool,

    /// Message callback.
    on_message: RwLock<Option<MessageCallback>>,

    /// Status callback.
    on_status: RwLock<Option<StatusCallback>>,

    /// Handle to the WebSocket task (for cancellation).
    task_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
}

impl TokioTransport {
    /// Create a new TokioTransport.
    pub fn new() -> Self {
        Self {
            sender: RwLock::new(None),
            status: RwLock::new(ConnectionStatus::Disconnected),
            connected: AtomicBool::new(false),
            on_message: RwLock::new(None),
            on_status: RwLock::new(None),
            task_handle: RwLock::new(None),
        }
    }

    /// Update status and notify callback.
    fn set_status(&self, status: ConnectionStatus) {
        {
            let mut s = self.status.write().unwrap();
            *s = status.clone();
        }
        if let Some(ref cb) = *self.on_status.read().unwrap() {
            cb(status);
        }
    }
}

impl Default for TokioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncTransport for TokioTransport {
    async fn connect(&self, config: &SyncConfig) -> Result<()> {
        if self.connected.load(Ordering::SeqCst) {
            return Ok(());
        }

        let url = config.build_url();
        log::info!("[TokioTransport] Connecting to {}", url);

        self.set_status(ConnectionStatus::Connecting);

        // Parse URL
        let parsed_url =
            url::Url::parse(&url).map_err(|e| DiaryxError::Crdt(format!("Invalid URL: {}", e)))?;

        // Connect to WebSocket
        let (ws_stream, _response) = tokio_tungstenite::connect_async(parsed_url.to_string())
            .await
            .map_err(|e| DiaryxError::Crdt(format!("WebSocket connection failed: {}", e)))?;

        log::info!("[TokioTransport] Connected");

        // Split the stream
        let (mut write, mut read) = ws_stream.split();

        // Create channel for outgoing messages
        let (tx, mut rx) = mpsc::unbounded_channel::<OutgoingMessage>();

        // Store the sender
        {
            let mut sender = self.sender.write().unwrap();
            *sender = Some(tx);
        }

        self.connected.store(true, Ordering::SeqCst);
        self.set_status(ConnectionStatus::Connected);

        // Get the message callback or use default
        let on_message: MessageCallback = self
            .on_message
            .read()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Arc::new(default_message_callback));

        let connected_flag = Arc::new(AtomicBool::new(true));
        let connected_flag_clone = Arc::clone(&connected_flag);

        // Spawn WebSocket task
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle incoming messages
                    msg = read.next() => {
                        match msg {
                            Some(Ok(Message::Binary(data))) => {
                                // Call the message callback
                                if let Some(response) = on_message(&data) {
                                    if let Err(e) = write.send(Message::Binary(response.into())).await {
                                        log::error!("[TokioTransport] Failed to send response: {}", e);
                                        break;
                                    }
                                }
                            }
                            Some(Ok(Message::Text(text))) => {
                                // Handle text messages (JSON control messages)
                                log::debug!("[TokioTransport] Received text: {}", text);
                            }
                            Some(Ok(Message::Ping(data))) => {
                                if let Err(e) = write.send(Message::Pong(data)).await {
                                    log::error!("[TokioTransport] Failed to send pong: {}", e);
                                    break;
                                }
                            }
                            Some(Ok(Message::Pong(_))) => {
                                // Pong received
                            }
                            Some(Ok(Message::Close(_))) => {
                                log::info!("[TokioTransport] Connection closed by server");
                                break;
                            }
                            Some(Ok(Message::Frame(_))) => {
                                // Raw frame, ignore
                            }
                            Some(Err(e)) => {
                                log::error!("[TokioTransport] WebSocket error: {}", e);
                                break;
                            }
                            None => {
                                log::info!("[TokioTransport] Stream ended");
                                break;
                            }
                        }
                    }
                    // Handle outgoing messages
                    msg = rx.recv() => {
                        match msg {
                            Some(OutgoingMessage::Binary(data)) => {
                                if let Err(e) = write.send(Message::Binary(data.into())).await {
                                    log::error!("[TokioTransport] Failed to send binary: {}", e);
                                    break;
                                }
                            }
                            Some(OutgoingMessage::Text(text)) => {
                                if let Err(e) = write.send(Message::Text(text.into())).await {
                                    log::error!("[TokioTransport] Failed to send text: {}", e);
                                    break;
                                }
                            }
                            None => {
                                // Channel closed
                                log::info!("[TokioTransport] Outgoing channel closed");
                                break;
                            }
                        }
                    }
                }
            }

            // Mark as disconnected
            connected_flag_clone.store(false, Ordering::SeqCst);

            // Close the WebSocket gracefully
            let _ = write.close().await;
        });

        // Store the task handle
        {
            let mut task = self.task_handle.write().unwrap();
            *task = Some(handle);
        }

        Ok(())
    }

    async fn send(&self, message: &[u8]) -> Result<()> {
        let sender = self.sender.read().unwrap();
        if let Some(ref tx) = *sender {
            tx.send(OutgoingMessage::Binary(message.to_vec()))
                .map_err(|e| DiaryxError::Crdt(format!("Failed to queue message: {}", e)))?;
            Ok(())
        } else {
            Err(DiaryxError::Crdt("Not connected".to_string()))
        }
    }

    async fn send_text(&self, message: &str) -> Result<()> {
        let sender = self.sender.read().unwrap();
        if let Some(ref tx) = *sender {
            tx.send(OutgoingMessage::Text(message.to_string()))
                .map_err(|e| DiaryxError::Crdt(format!("Failed to queue text message: {}", e)))?;
            Ok(())
        } else {
            Err(DiaryxError::Crdt("Not connected".to_string()))
        }
    }

    fn set_on_message(&self, callback: MessageCallback) {
        let mut cb = self.on_message.write().unwrap();
        *cb = Some(callback);
    }

    fn set_on_status(&self, callback: StatusCallback) {
        let mut cb = self.on_status.write().unwrap();
        *cb = Some(callback);
    }

    async fn disconnect(&self) -> Result<()> {
        log::info!("[TokioTransport] Disconnecting");

        // Clear the sender to stop accepting new messages
        {
            let mut sender = self.sender.write().unwrap();
            *sender = None;
        }

        // Abort the task
        {
            let mut task = self.task_handle.write().unwrap();
            if let Some(handle) = task.take() {
                handle.abort();
            }
        }

        self.connected.store(false, Ordering::SeqCst);
        self.set_status(ConnectionStatus::Disconnected);

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn status(&self) -> ConnectionStatus {
        self.status.read().unwrap().clone()
    }
}

impl std::fmt::Debug for TokioTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokioTransport")
            .field("connected", &self.connected.load(Ordering::SeqCst))
            .field("status", &self.status())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokio_transport_new() {
        let transport = TokioTransport::new();
        assert!(!transport.is_connected());
        assert_eq!(transport.status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_tokio_transport_default() {
        let transport = TokioTransport::default();
        assert!(!transport.is_connected());
    }
}
