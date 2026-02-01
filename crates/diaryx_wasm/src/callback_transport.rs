//! Callback-based transport for WASM/JavaScript WebSocket integration.
//!
//! This module provides `CallbackTransport`, which implements the `SyncTransport`
//! trait using message queues that JavaScript can interact with. Since WASM can't
//! directly create WebSocket connections, this transport:
//!
//! 1. Queues outgoing messages for JavaScript to poll and send
//! 2. Accepts incoming messages injected by JavaScript
//! 3. Routes messages to the appropriate callbacks
//!
//! # JavaScript Integration
//!
//! ```javascript
//! // Set up WebSocket in JavaScript
//! const ws = new WebSocket(url);
//! ws.binaryType = 'arraybuffer';
//!
//! // Forward incoming messages to WASM
//! ws.onmessage = (event) => {
//!   const data = new Uint8Array(event.data);
//!   const response = backend.injectSyncMessage(data);
//!   if (response) ws.send(response);
//! };
//!
//! // Poll for outgoing messages and send them
//! setInterval(() => {
//!   while (true) {
//!     const msg = backend.pollOutgoingMessage();
//!     if (!msg) break;
//!     ws.send(msg);
//!   }
//! }, 10);
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_core::crdt::{
    ConnectionStatus, MessageCallback, StatusCallback, SyncConfig, SyncTransport,
};
use diaryx_core::error::Result;

/// Callback-based sync transport for WASM.
///
/// This transport works by:
/// 1. Queueing outgoing messages for JavaScript to poll
/// 2. Processing incoming messages injected by JavaScript
/// 3. Invoking callbacks to handle messages and status changes
///
/// # Thread Safety
///
/// This transport uses `RefCell` because WASM is single-threaded.
/// All operations happen on the main JS thread.
pub struct CallbackTransport {
    /// Queue of outgoing messages for JavaScript to send.
    outgoing: RefCell<VecDeque<Vec<u8>>>,

    /// Callback for incoming messages.
    on_message: RefCell<Option<MessageCallback>>,

    /// Callback for status changes.
    on_status: RefCell<Option<StatusCallback>>,

    /// Current connection status.
    status: RefCell<ConnectionStatus>,

    /// Whether "connected" (from JS perspective).
    connected: AtomicBool,

    /// Current sync configuration.
    config: RefCell<Option<SyncConfig>>,
}

impl CallbackTransport {
    /// Create a new CallbackTransport.
    pub fn new() -> Self {
        Self {
            outgoing: RefCell::new(VecDeque::new()),
            on_message: RefCell::new(None),
            on_status: RefCell::new(None),
            status: RefCell::new(ConnectionStatus::Disconnected),
            connected: AtomicBool::new(false),
            config: RefCell::new(None),
        }
    }

    /// Inject an incoming message from JavaScript.
    ///
    /// This is called by JavaScript when a WebSocket message is received.
    /// Returns an optional response message to send back.
    pub fn inject_message(&self, message: &[u8]) -> Option<Vec<u8>> {
        let callback = self.on_message.borrow();
        if let Some(ref cb) = *callback {
            cb(message)
        } else {
            None
        }
    }

    /// Poll for an outgoing message to send via JavaScript.
    ///
    /// Returns the next message in the queue, or None if empty.
    /// JavaScript should call this repeatedly until it returns None.
    pub fn poll_outgoing(&self) -> Option<Vec<u8>> {
        self.outgoing.borrow_mut().pop_front()
    }

    /// Queue an outgoing message.
    ///
    /// Called internally when `send()` is invoked.
    fn queue_outgoing(&self, message: Vec<u8>) {
        self.outgoing.borrow_mut().push_back(message);
    }

    /// Update status and notify callback.
    fn set_status(&self, status: ConnectionStatus) {
        *self.status.borrow_mut() = status.clone();
        if let Some(ref cb) = *self.on_status.borrow() {
            cb(status);
        }
    }

    /// Mark as connected (called by JavaScript when WebSocket opens).
    pub fn mark_connected(&self) {
        self.connected.store(true, Ordering::SeqCst);
        self.set_status(ConnectionStatus::Connected);
    }

    /// Mark as disconnected (called by JavaScript when WebSocket closes).
    pub fn mark_disconnected(&self) {
        self.connected.store(false, Ordering::SeqCst);
        self.set_status(ConnectionStatus::Disconnected);
    }

    /// Get the URL to connect to (for JavaScript).
    pub fn get_connect_url(&self) -> Option<String> {
        self.config.borrow().as_ref().map(|c| c.build_url())
    }

    /// Check if there are pending outgoing messages.
    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.borrow().is_empty()
    }

    /// Get the number of pending outgoing messages.
    pub fn outgoing_count(&self) -> usize {
        self.outgoing.borrow().len()
    }

    /// Clear all outgoing messages.
    pub fn clear_outgoing(&self) {
        self.outgoing.borrow_mut().clear();
    }
}

impl Default for CallbackTransport {
    fn default() -> Self {
        Self::new()
    }
}

// Note: CallbackTransport is NOT Send + Sync because it uses RefCell.
// This is fine for WASM which is single-threaded.
// The SyncTransport trait requires Send + Sync for native, but WASM
// implementations can bypass this with unsafe (or we use a different trait).

// For WASM, we provide a simpler synchronous interface since we can't
// use async in the same way as native. The trait implementation is
// provided for API consistency, but WASM users typically call the
// inject_message and poll_outgoing methods directly.

impl SyncTransport for CallbackTransport {
    async fn connect(&self, config: &SyncConfig) -> Result<()> {
        // Store config for JavaScript to retrieve the URL
        *self.config.borrow_mut() = Some(config.clone());
        self.set_status(ConnectionStatus::Connecting);
        // Actual connection is done by JavaScript
        // JavaScript should call mark_connected() when WebSocket opens
        Ok(())
    }

    async fn send(&self, message: &[u8]) -> Result<()> {
        self.queue_outgoing(message.to_vec());
        Ok(())
    }

    fn set_on_message(&self, callback: MessageCallback) {
        *self.on_message.borrow_mut() = Some(callback);
    }

    fn set_on_status(&self, callback: StatusCallback) {
        *self.on_status.borrow_mut() = Some(callback);
    }

    async fn disconnect(&self) -> Result<()> {
        self.clear_outgoing();
        self.connected.store(false, Ordering::SeqCst);
        self.set_status(ConnectionStatus::Disconnected);
        *self.config.borrow_mut() = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn status(&self) -> ConnectionStatus {
        self.status.borrow().clone()
    }
}

// SAFETY: CallbackTransport uses RefCell which is not thread-safe, but
// WASM is single-threaded so this is safe. We implement Send + Sync
// to satisfy the SyncTransport trait bounds.
unsafe impl Send for CallbackTransport {}
unsafe impl Sync for CallbackTransport {}

impl std::fmt::Debug for CallbackTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackTransport")
            .field("connected", &self.connected.load(Ordering::SeqCst))
            .field("status", &*self.status.borrow())
            .field("outgoing_count", &self.outgoing.borrow().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_callback_transport_new() {
        let transport = CallbackTransport::new();
        assert!(!transport.is_connected());
        assert_eq!(transport.status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_outgoing_queue() {
        let transport = CallbackTransport::new();

        // Queue some messages
        transport.queue_outgoing(vec![1, 2, 3]);
        transport.queue_outgoing(vec![4, 5, 6]);

        assert!(transport.has_outgoing());
        assert_eq!(transport.outgoing_count(), 2);

        // Poll messages in order
        assert_eq!(transport.poll_outgoing(), Some(vec![1, 2, 3]));
        assert_eq!(transport.poll_outgoing(), Some(vec![4, 5, 6]));
        assert_eq!(transport.poll_outgoing(), None);

        assert!(!transport.has_outgoing());
    }

    #[test]
    fn test_inject_message() {
        let transport = CallbackTransport::new();

        // Set up callback that echoes messages back
        let callback: MessageCallback = Arc::new(|msg: &[u8]| Some(msg.to_vec()));
        transport.set_on_message(callback);

        // Inject message
        let response = transport.inject_message(&[1, 2, 3]);
        assert_eq!(response, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_inject_message_no_callback() {
        let transport = CallbackTransport::new();

        // No callback set
        let response = transport.inject_message(&[1, 2, 3]);
        assert_eq!(response, None);
    }

    #[test]
    fn test_connection_status() {
        let transport = CallbackTransport::new();

        assert!(!transport.is_connected());
        assert_eq!(transport.status(), ConnectionStatus::Disconnected);

        transport.mark_connected();
        assert!(transport.is_connected());
        assert_eq!(transport.status(), ConnectionStatus::Connected);

        transport.mark_disconnected();
        assert!(!transport.is_connected());
        assert_eq!(transport.status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_get_connect_url() {
        let transport = CallbackTransport::new();

        // No config yet
        assert_eq!(transport.get_connect_url(), None);

        // Set config via connect (sync for test)
        let config = SyncConfig::metadata(
            "wss://sync.example.com".to_string(),
            "workspace".to_string(),
        );
        *transport.config.borrow_mut() = Some(config);

        let url = transport.get_connect_url();
        assert!(url.is_some());
        assert!(url.unwrap().contains("doc=workspace"));
    }
}
