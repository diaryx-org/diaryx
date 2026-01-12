//! Y-sync protocol implementation for Hocuspocus compatibility.
//!
//! This module provides the sync protocol layer for communicating with
//! Hocuspocus servers. It handles the Y-sync message encoding/decoding
//! and state synchronization.
//!
//! # Protocol Overview
//!
//! The Y-sync protocol uses a two-phase handshake:
//!
//! 1. **SyncStep1**: Client sends its state vector to the server
//! 2. **SyncStep2**: Server responds with missing updates
//!
//! After the handshake, updates are exchanged bidirectionally.
//!
//! # Wire Format
//!
//! Messages are prefixed with a message type byte:
//! - `0`: Sync message (followed by sync type: 0=Step1, 1=Step2, 2=Update)
//! - `1`: Awareness message
//! - `2`: Auth message
//!
//! # Example
//!
//! ```ignore
//! use diaryx_core::crdt::{SyncProtocol, WorkspaceCrdt, MemoryStorage};
//! use std::sync::Arc;
//!
//! let storage = Arc::new(MemoryStorage::new());
//! let workspace = WorkspaceCrdt::new(storage);
//! let mut protocol = SyncProtocol::new(workspace);
//!
//! // Generate initial sync message (SyncStep1)
//! let sync_step1 = protocol.create_sync_step1();
//!
//! // Handle response from server
//! if let Some(response) = protocol.handle_message(&server_message)? {
//!     // Send response back to server
//! }
//! ```

use yrs::{ReadTxn, Transact, Update, updates::decoder::Decode, updates::encoder::Encode};

use super::storage::StorageResult;
use super::types::UpdateOrigin;
use super::workspace_doc::WorkspaceCrdt;
use crate::error::DiaryxError;

/// Message type bytes for the Y-sync protocol.
mod msg_type {
    /// Sync message (SyncStep1, SyncStep2, Update)
    pub const SYNC: u8 = 0;
    /// Awareness message
    pub const AWARENESS: u8 = 1;
    /// Auth message (reserved for future use)
    #[allow(dead_code)]
    pub const AUTH: u8 = 2;
}

/// Sync sub-message types.
mod sync_type {
    /// SyncStep1: Initial state vector exchange
    pub const STEP1: u8 = 0;
    /// SyncStep2: Missing updates response
    pub const STEP2: u8 = 1;
    /// Update: Incremental update
    pub const UPDATE: u8 = 2;
}

/// Y-sync message types.
#[derive(Debug, Clone)]
pub enum SyncMessage {
    /// SyncStep1 contains a state vector
    SyncStep1(Vec<u8>),
    /// SyncStep2 contains missing updates
    SyncStep2(Vec<u8>),
    /// Update contains an incremental update
    Update(Vec<u8>),
}

impl SyncMessage {
    /// Encode the message to bytes.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            SyncMessage::SyncStep1(sv) => {
                let mut buf = Vec::with_capacity(2 + sv.len());
                buf.push(msg_type::SYNC);
                buf.push(sync_type::STEP1);
                buf.extend_from_slice(sv);
                buf
            }
            SyncMessage::SyncStep2(update) => {
                let mut buf = Vec::with_capacity(2 + update.len());
                buf.push(msg_type::SYNC);
                buf.push(sync_type::STEP2);
                buf.extend_from_slice(update);
                buf
            }
            SyncMessage::Update(update) => {
                let mut buf = Vec::with_capacity(2 + update.len());
                buf.push(msg_type::SYNC);
                buf.push(sync_type::UPDATE);
                buf.extend_from_slice(update);
                buf
            }
        }
    }

    /// Decode a message from bytes.
    pub fn decode(data: &[u8]) -> StorageResult<Option<Self>> {
        if data.len() < 2 {
            return Ok(None);
        }

        let msg_type = data[0];
        if msg_type != msg_type::SYNC {
            // Non-sync message (awareness, auth), skip it
            return Ok(None);
        }

        let sync_type = data[1];
        let payload = &data[2..];

        match sync_type {
            sync_type::STEP1 => Ok(Some(SyncMessage::SyncStep1(payload.to_vec()))),
            sync_type::STEP2 => Ok(Some(SyncMessage::SyncStep2(payload.to_vec()))),
            sync_type::UPDATE => Ok(Some(SyncMessage::Update(payload.to_vec()))),
            _ => Err(DiaryxError::Crdt(format!(
                "Unknown sync type: {}",
                sync_type
            ))),
        }
    }
}

/// Sync protocol handler for a workspace CRDT.
///
/// This struct manages the Y-sync protocol state and message handling
/// for synchronizing a workspace CRDT with a remote server.
pub struct SyncProtocol {
    workspace: WorkspaceCrdt,
}

impl SyncProtocol {
    /// Create a new sync protocol handler.
    pub fn new(workspace: WorkspaceCrdt) -> Self {
        Self { workspace }
    }

    /// Get a reference to the underlying workspace CRDT.
    pub fn workspace(&self) -> &WorkspaceCrdt {
        &self.workspace
    }

    /// Get a mutable reference to the underlying workspace CRDT.
    pub fn workspace_mut(&mut self) -> &mut WorkspaceCrdt {
        &mut self.workspace
    }

    /// Create a SyncStep1 message containing the local state vector.
    ///
    /// This is the first message sent to initiate synchronization.
    /// The server will respond with a SyncStep2 containing missing updates.
    pub fn create_sync_step1(&self) -> Vec<u8> {
        let sv = self.workspace.encode_state_vector();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Create a SyncStep2 message with updates the remote peer is missing.
    ///
    /// This is sent in response to a SyncStep1 from a remote peer.
    pub fn create_sync_step2(&self, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let diff = self.workspace.encode_diff(remote_state_vector)?;
        Ok(SyncMessage::SyncStep2(diff).encode())
    }

    /// Create an update message for broadcasting local changes.
    ///
    /// Use this to send local changes to remote peers after the initial sync.
    pub fn create_update_message(&self, update: &[u8]) -> Vec<u8> {
        SyncMessage::Update(update.to_vec()).encode()
    }

    /// Handle an incoming message from a remote peer.
    ///
    /// Returns an optional response message that should be sent back.
    ///
    /// # Message Types
    ///
    /// - **SyncStep1**: Returns a SyncStep2 with missing updates
    /// - **SyncStep2**: Applies the update, returns None
    /// - **Update**: Applies the update, returns None
    pub fn handle_message(&mut self, msg: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let Some(sync_msg) = SyncMessage::decode(msg)? else {
            // Non-sync message, ignore
            return Ok(None);
        };

        match sync_msg {
            SyncMessage::SyncStep1(remote_sv) => {
                // Remote is requesting updates they don't have
                let response = self.create_sync_step2(&remote_sv)?;

                // Also send our state vector back so they can send us what we're missing
                let our_sv = self.workspace.encode_state_vector();
                let our_step1 = SyncMessage::SyncStep1(our_sv).encode();

                // Combine responses
                let mut combined = response;
                combined.extend_from_slice(&our_step1);

                Ok(Some(combined))
            }
            SyncMessage::SyncStep2(update) => {
                // Remote is sending updates we don't have
                if !update.is_empty() {
                    self.workspace.apply_update(&update, UpdateOrigin::Sync)?;
                }
                Ok(None)
            }
            SyncMessage::Update(update) => {
                // Remote is sending a live update
                if !update.is_empty() {
                    self.workspace.apply_update(&update, UpdateOrigin::Remote)?;
                }
                Ok(None)
            }
        }
    }

    /// Get the current state as a full update.
    ///
    /// Useful for getting the complete state to send to a new peer.
    pub fn get_full_state(&self) -> Vec<u8> {
        self.workspace.encode_state_as_update()
    }

    /// Apply a raw update from any source.
    pub fn apply_update(&mut self, update: &[u8], origin: UpdateOrigin) -> StorageResult<()> {
        self.workspace.apply_update(update, origin)?;
        Ok(())
    }
}

impl std::fmt::Debug for SyncProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncProtocol")
            .field("workspace", &self.workspace)
            .finish()
    }
}

/// Sync protocol handler for a body document.
///
/// Similar to `SyncProtocol` but for individual file body documents.
pub struct BodySyncProtocol {
    doc_name: String,
    doc: yrs::Doc,
}

impl BodySyncProtocol {
    /// Create a new body sync protocol handler.
    pub fn new(doc_name: String) -> Self {
        Self {
            doc_name,
            doc: yrs::Doc::new(),
        }
    }

    /// Create from an existing document state.
    pub fn from_state(doc_name: String, state: &[u8]) -> StorageResult<Self> {
        let doc = yrs::Doc::new();
        if !state.is_empty() {
            let update = Update::decode_v1(state)
                .map_err(|e| DiaryxError::Crdt(format!("Failed to decode state: {}", e)))?;
            let mut txn = doc.transact_mut();
            txn.apply_update(update)
                .map_err(|e| DiaryxError::Crdt(format!("Failed to apply state: {}", e)))?;
        }
        Ok(Self { doc_name, doc })
    }

    /// Get the document name.
    pub fn doc_name(&self) -> &str {
        &self.doc_name
    }

    /// Create a SyncStep1 message.
    pub fn create_sync_step1(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        let sv = txn.state_vector().encode_v1();
        SyncMessage::SyncStep1(sv).encode()
    }

    /// Create a SyncStep2 message.
    pub fn create_sync_step2(&self, remote_state_vector: &[u8]) -> StorageResult<Vec<u8>> {
        let sv = yrs::StateVector::decode_v1(remote_state_vector)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode state vector: {}", e)))?;

        let txn = self.doc.transact();
        let diff = txn.encode_state_as_update_v1(&sv);

        Ok(SyncMessage::SyncStep2(diff).encode())
    }

    /// Create an update message.
    pub fn create_update_message(&self, update: &[u8]) -> Vec<u8> {
        SyncMessage::Update(update.to_vec()).encode()
    }

    /// Handle an incoming message.
    pub fn handle_message(&mut self, msg: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let Some(sync_msg) = SyncMessage::decode(msg)? else {
            return Ok(None);
        };

        match sync_msg {
            SyncMessage::SyncStep1(remote_sv) => {
                let response = self.create_sync_step2(&remote_sv)?;

                // Also send our state vector
                let txn = self.doc.transact();
                let our_sv = txn.state_vector().encode_v1();
                drop(txn);

                let our_step1 = SyncMessage::SyncStep1(our_sv).encode();

                let mut combined = response;
                combined.extend_from_slice(&our_step1);

                Ok(Some(combined))
            }
            SyncMessage::SyncStep2(update) => {
                if !update.is_empty() {
                    self.apply_update(&update)?;
                }
                Ok(None)
            }
            SyncMessage::Update(update) => {
                if !update.is_empty() {
                    self.apply_update(&update)?;
                }
                Ok(None)
            }
        }
    }

    /// Apply an update to the document.
    pub fn apply_update(&mut self, update: &[u8]) -> StorageResult<()> {
        let decoded = Update::decode_v1(update)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to decode update: {}", e)))?;
        let mut txn = self.doc.transact_mut();
        txn.apply_update(decoded)
            .map_err(|e| DiaryxError::Crdt(format!("Failed to apply update: {}", e)))?;
        Ok(())
    }

    /// Get the full state as an update.
    pub fn get_full_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&Default::default())
    }

    /// Get the state vector.
    pub fn get_state_vector(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.state_vector().encode_v1()
    }
}

impl std::fmt::Debug for BodySyncProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BodySyncProtocol")
            .field("doc_name", &self.doc_name)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt::MemoryStorage;
    use std::sync::Arc;
    use yrs::{GetString, Text};

    fn create_sync_protocol() -> SyncProtocol {
        let storage = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage);
        SyncProtocol::new(workspace)
    }

    #[test]
    fn test_sync_message_encode_decode() {
        let sv = vec![1, 2, 3, 4];
        let msg = SyncMessage::SyncStep1(sv.clone());
        let encoded = msg.encode();

        let decoded = SyncMessage::decode(&encoded).unwrap().unwrap();
        match decoded {
            SyncMessage::SyncStep1(decoded_sv) => assert_eq!(decoded_sv, sv),
            _ => panic!("Expected SyncStep1"),
        }
    }

    #[test]
    fn test_sync_message_step2() {
        let update = vec![5, 6, 7, 8];
        let msg = SyncMessage::SyncStep2(update.clone());
        let encoded = msg.encode();

        let decoded = SyncMessage::decode(&encoded).unwrap().unwrap();
        match decoded {
            SyncMessage::SyncStep2(decoded_update) => assert_eq!(decoded_update, update),
            _ => panic!("Expected SyncStep2"),
        }
    }

    #[test]
    fn test_sync_message_update() {
        let update = vec![9, 10, 11, 12];
        let msg = SyncMessage::Update(update.clone());
        let encoded = msg.encode();

        let decoded = SyncMessage::decode(&encoded).unwrap().unwrap();
        match decoded {
            SyncMessage::Update(decoded_update) => assert_eq!(decoded_update, update),
            _ => panic!("Expected Update"),
        }
    }

    #[test]
    fn test_create_sync_step1() {
        let protocol = create_sync_protocol();
        let step1 = protocol.create_sync_step1();

        // Should start with sync message type and step1 subtype
        assert!(step1.len() >= 2);
        assert_eq!(step1[0], msg_type::SYNC);
        assert_eq!(step1[1], sync_type::STEP1);
    }

    #[test]
    fn test_sync_between_protocols() {
        let storage1: Arc<dyn crate::crdt::CrdtStorage> = Arc::new(MemoryStorage::new());
        let storage2: Arc<dyn crate::crdt::CrdtStorage> = Arc::new(MemoryStorage::new());

        let workspace1 = WorkspaceCrdt::new(storage1);
        let workspace2 = WorkspaceCrdt::new(storage2);

        let mut protocol1 = SyncProtocol::new(workspace1);
        let mut protocol2 = SyncProtocol::new(workspace2);

        // Add data to protocol1
        let metadata = crate::crdt::FileMetadata::new(Some("Test File".to_string()));
        protocol1.workspace_mut().set_file("test.md", metadata);

        // Initiate sync: protocol1 -> protocol2
        let step1 = protocol1.create_sync_step1();
        let response = protocol2.handle_message(&step1).unwrap();

        // Protocol2 should respond
        assert!(response.is_some());

        // Protocol1 handles response (which contains SyncStep2 + SyncStep1)
        if let Some(resp) = response {
            // The response contains multiple messages, handle them
            // First message is SyncStep2 (skip 2 bytes header + data)
            // Second message is SyncStep1
            let _ = protocol1.handle_message(&resp);
        }

        // Now sync protocol2 changes to protocol1
        let step1_2 = protocol2.create_sync_step1();
        let response_2 = protocol1.handle_message(&step1_2).unwrap();

        if let Some(resp) = response_2 {
            let _ = protocol2.handle_message(&resp);
        }

        // Both should have the file now
        assert!(protocol2.workspace().get_file("test.md").is_some());
    }

    #[test]
    fn test_update_message() {
        let storage: Arc<dyn crate::crdt::CrdtStorage> = Arc::new(MemoryStorage::new());
        let workspace = WorkspaceCrdt::new(storage);
        let mut protocol = SyncProtocol::new(workspace);

        // Make a change
        let metadata = crate::crdt::FileMetadata::new(Some("New File".to_string()));
        protocol.workspace_mut().set_file("new.md", metadata);

        // Get the state as an update
        let state = protocol.get_full_state();

        // Create an update message
        let update_msg = protocol.create_update_message(&state);

        // Should be decodable
        let decoded = SyncMessage::decode(&update_msg).unwrap().unwrap();
        match decoded {
            SyncMessage::Update(_) => {}
            _ => panic!("Expected Update message"),
        }
    }

    #[test]
    fn test_body_sync_protocol() {
        let mut protocol1 = BodySyncProtocol::new("test.md".to_string());
        let mut protocol2 = BodySyncProtocol::new("test.md".to_string());

        // Add content to protocol1
        {
            let text = protocol1.doc.get_or_insert_text("body");
            let mut txn = protocol1.doc.transact_mut();
            text.insert(&mut txn, 0, "Hello from protocol1");
        }

        // Sync
        let step1 = protocol1.create_sync_step1();
        let response = protocol2.handle_message(&step1).unwrap();

        if let Some(resp) = response {
            let _ = protocol1.handle_message(&resp);
        }

        // Sync back
        let step1_2 = protocol2.create_sync_step1();
        let response_2 = protocol1.handle_message(&step1_2).unwrap();

        if let Some(resp) = response_2 {
            let _ = protocol2.handle_message(&resp);
        }

        // Check content synced
        let text2 = protocol2.doc.get_or_insert_text("body");
        let txn = protocol2.doc.transact();
        let content = text2.get_string(&txn);
        assert_eq!(content, "Hello from protocol1");
    }

    #[test]
    fn test_body_sync_from_state() {
        let protocol1 = BodySyncProtocol::new("test.md".to_string());

        // Add content
        {
            let text = protocol1.doc.get_or_insert_text("body");
            let mut txn = protocol1.doc.transact_mut();
            text.insert(&mut txn, 0, "Persisted content");
        }

        // Get state
        let state = protocol1.get_full_state();

        // Create new protocol from state
        let protocol2 = BodySyncProtocol::from_state("test.md".to_string(), &state).unwrap();

        let text2 = protocol2.doc.get_or_insert_text("body");
        let txn = protocol2.doc.transact();
        let content = text2.get_string(&txn);
        assert_eq!(content, "Persisted content");
    }

    #[test]
    fn test_empty_state() {
        let protocol = BodySyncProtocol::from_state("empty.md".to_string(), &[]).unwrap();
        assert_eq!(protocol.doc_name(), "empty.md");
    }

    #[test]
    fn test_non_sync_message_ignored() {
        let mut protocol = create_sync_protocol();

        // Create a non-sync message (awareness type)
        let fake_awareness = vec![msg_type::AWARENESS, 0, 1, 2, 3];
        let result = protocol.handle_message(&fake_awareness).unwrap();

        // Should return None (ignored)
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_message() {
        let result = SyncMessage::decode(&[]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_short_message() {
        let result = SyncMessage::decode(&[0]).unwrap();
        assert!(result.is_none());
    }
}
