//! Transport abstraction for sync connections.
//!
//! This module defines the `SyncTransport` trait that provides a unified interface
//! for WebSocket-based sync connections across all platforms:
//!
//! - **Native (CLI, Tauri)**: Uses `TokioTransport` with tokio-tungstenite
//! - **WASM (Web)**: Uses `CallbackTransport` with JavaScript WebSocket via callbacks
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────┐    ┌────────────────────┐
//! │ TokioTransport     │    │ CallbackTransport  │
//! │ (tokio-tungstenite)│    │ (JS WebSocket)     │
//! │ #[cfg(native)]     │    │ #[cfg(wasm32)]     │
//! └─────────┬──────────┘    └─────────┬──────────┘
//!           │                         │
//!           └────────────┬────────────┘
//!                        ▼
//!           ┌──────────────────────┐
//!           │   SyncClient<T>      │
//!           │   - Reconnection     │
//!           │   - Dual connections │
//!           │   - Message routing  │
//!           └──────────────────────┘
//!                        │
//!                        ▼
//!           ┌──────────────────────┐
//!           │   RustSyncManager    │
//!           └──────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{SyncTransport, SyncConfig};
//!
//! // Native: Use TokioTransport
//! #[cfg(not(target_arch = "wasm32"))]
//! let transport = TokioTransport::new();
//!
//! // WASM: Use CallbackTransport (messages routed via JS)
//! #[cfg(target_arch = "wasm32")]
//! let transport = CallbackTransport::new();
//!
//! // Both implement SyncTransport
//! transport.connect(&config).await?;
//! transport.send(b"sync message").await?;
//! ```

use crate::error::Result;
use std::sync::Arc;

/// Configuration for a sync connection.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// WebSocket server URL (e.g., "wss://sync.diaryx.org/sync").
    pub server_url: String,

    /// Document/workspace ID for the sync session.
    pub doc_id: String,

    /// Optional authentication token.
    pub auth_token: Option<String>,

    /// Whether this is a multiplexed body connection (vs metadata).
    pub multiplexed: bool,

    /// Whether to write synced changes to disk.
    pub write_to_disk: bool,
}

impl SyncConfig {
    /// Create a new sync configuration for metadata sync.
    pub fn metadata(server_url: String, doc_id: String) -> Self {
        Self {
            server_url,
            doc_id,
            auth_token: None,
            multiplexed: false,
            write_to_disk: true,
        }
    }

    /// Create a new sync configuration for multiplexed body sync.
    pub fn body(server_url: String, doc_id: String) -> Self {
        Self {
            server_url,
            doc_id,
            auth_token: None,
            multiplexed: true,
            write_to_disk: true,
        }
    }

    /// Set the authentication token.
    pub fn with_auth(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// Set whether to write changes to disk.
    pub fn with_write_to_disk(mut self, write: bool) -> Self {
        self.write_to_disk = write;
        self
    }

    /// Build the WebSocket URL with query parameters.
    pub fn build_url(&self) -> String {
        let mut url = self.server_url.clone();

        // Add doc parameter
        if !url.contains('?') {
            url.push_str("?doc=");
        } else {
            url.push_str("&doc=");
        }
        url.push_str(&self.doc_id);

        // Add multiplexed flag for body sync
        if self.multiplexed {
            url.push_str("&multiplexed=true");
        }

        // Add auth token if provided
        if let Some(ref token) = self.auth_token {
            url.push_str("&token=");
            url.push_str(token);
        }

        url
    }
}

/// Status of a sync connection.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectionStatus {
    /// Not connected to the server.
    Disconnected,
    /// Currently connecting to the server.
    Connecting,
    /// Connected and ready for sync.
    Connected,
    /// Performing initial sync handshake.
    Syncing {
        /// Number of files synced so far.
        completed: usize,
        /// Total number of files to sync.
        total: usize,
    },
    /// Initial sync complete, watching for changes.
    Synced,
    /// Reconnecting after disconnection.
    Reconnecting {
        /// Current reconnection attempt number.
        attempt: u32,
    },
    /// Connection error occurred.
    Error {
        /// Error message describing what went wrong.
        message: String,
    },
}

/// Callback type for handling incoming sync messages.
///
/// The callback receives the raw message bytes and returns an optional
/// response to send back to the server.
pub type MessageCallback = Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>;

/// Callback type for connection status changes.
pub type StatusCallback = Arc<dyn Fn(ConnectionStatus) + Send + Sync>;

/// Transport abstraction for sync connections.
///
/// This trait provides a unified interface for WebSocket-based sync connections.
/// Implementations handle the platform-specific WebSocket details:
///
/// - **Native**: `TokioTransport` uses tokio-tungstenite
/// - **WASM**: `CallbackTransport` uses JavaScript WebSocket via callbacks
///
/// # Message Flow
///
/// 1. Call `connect()` to establish the WebSocket connection
/// 2. Set `on_message` callback to handle incoming messages
/// 3. Call `send()` to send outgoing messages
/// 4. Call `disconnect()` when done
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to allow use across async contexts.
pub trait SyncTransport: Send + Sync {
    /// Connect to the sync server.
    ///
    /// Establishes a WebSocket connection using the provided configuration.
    /// Returns when the connection is established or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails (network error, auth failure, etc.).
    fn connect(&self, config: &SyncConfig) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Send a binary message to the server.
    ///
    /// The message is sent as a WebSocket binary frame.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection is closed or the send fails.
    fn send(&self, message: &[u8]) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Set the callback for incoming messages.
    ///
    /// The callback receives raw message bytes and returns an optional response
    /// to send back to the server immediately.
    ///
    /// # Note
    ///
    /// Only one callback can be active at a time. Setting a new callback
    /// replaces the previous one.
    fn set_on_message(&self, callback: MessageCallback);

    /// Set the callback for status changes.
    ///
    /// The callback is invoked whenever the connection status changes.
    fn set_on_status(&self, callback: StatusCallback);

    /// Disconnect from the server.
    ///
    /// Closes the WebSocket connection gracefully. After disconnecting,
    /// the transport can be reconnected by calling `connect()` again.
    fn disconnect(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Check if currently connected.
    fn is_connected(&self) -> bool;

    /// Get the current connection status.
    fn status(&self) -> ConnectionStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_metadata() {
        let config = SyncConfig::metadata(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        );

        assert_eq!(config.server_url, "wss://sync.example.com/sync");
        assert_eq!(config.doc_id, "workspace123");
        assert!(!config.multiplexed);
        assert!(config.write_to_disk);
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_sync_config_body() {
        let config = SyncConfig::body(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        );

        assert!(config.multiplexed);
    }

    #[test]
    fn test_sync_config_with_auth() {
        let config = SyncConfig::metadata(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        )
        .with_auth("token123".to_string());

        assert_eq!(config.auth_token, Some("token123".to_string()));
    }

    #[test]
    fn test_build_url_metadata() {
        let config = SyncConfig::metadata(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        );

        let url = config.build_url();
        assert_eq!(url, "wss://sync.example.com/sync?doc=workspace123");
    }

    #[test]
    fn test_build_url_body() {
        let config = SyncConfig::body(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        );

        let url = config.build_url();
        assert!(url.contains("doc=workspace123"));
        assert!(url.contains("multiplexed=true"));
    }

    #[test]
    fn test_build_url_with_auth() {
        let config = SyncConfig::metadata(
            "wss://sync.example.com/sync".to_string(),
            "workspace123".to_string(),
        )
        .with_auth("mytoken".to_string());

        let url = config.build_url();
        assert!(url.contains("token=mytoken"));
    }

    #[test]
    fn test_build_url_existing_query_params() {
        let config = SyncConfig::metadata(
            "wss://sync.example.com/sync?version=1".to_string(),
            "workspace123".to_string(),
        );

        let url = config.build_url();
        assert!(url.contains("version=1"));
        assert!(url.contains("&doc=workspace123"));
    }

    #[test]
    fn test_connection_status_variants() {
        let status = ConnectionStatus::Disconnected;
        assert_eq!(status, ConnectionStatus::Disconnected);

        let status = ConnectionStatus::Syncing {
            completed: 5,
            total: 10,
        };
        assert_eq!(
            status,
            ConnectionStatus::Syncing {
                completed: 5,
                total: 10
            }
        );

        let status = ConnectionStatus::Error {
            message: "timeout".to_string(),
        };
        assert_eq!(
            status,
            ConnectionStatus::Error {
                message: "timeout".to_string()
            }
        );
    }
}
