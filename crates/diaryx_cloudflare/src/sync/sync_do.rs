//! Durable Object for namespace sync.
//!
//! One DO per namespace. Manages WebSocket connections for Y-sync protocol.
//! Uses the WebSocket Hibernation API for cost-efficient long-lived connections.

use super::do_storage::DoSyncStorage;
use diaryx_sync::protocol_types::{DocType, ServerControlMessage};
use diaryx_sync::{SyncDocManager, UpdateOrigin};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use worker::*;

/// Per-connection metadata, persisted through hibernation via serialize_attachment.
#[derive(Clone, Serialize, Deserialize)]
struct ConnectionMeta {
    user_id: String,
    workspace_id: String,
    device_id: Option<String>,
    is_guest: bool,
    read_only: bool,
    handshake_complete: bool,
}

/// NamespaceSyncDO — one Durable Object per namespace.
#[durable_object]
pub struct NamespaceSyncDO {
    state: State,
    env: Env,
}

impl DurableObject for NamespaceSyncDO {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        // Handle WebSocket upgrade
        if req.headers().get("Upgrade")?.as_deref() == Some("websocket") {
            return self.handle_websocket_upgrade(req).await;
        }

        Response::error("Expected WebSocket upgrade", 426)
    }

    async fn websocket_message(
        &self,
        ws: WebSocket,
        message: WebSocketIncomingMessage,
    ) -> Result<()> {
        let meta = self.get_connection_meta(&ws)?;

        match message {
            WebSocketIncomingMessage::Binary(data) => {
                self.handle_binary_message(&ws, &meta, data)?;
            }
            WebSocketIncomingMessage::String(text) => {
                self.handle_text_message(&ws, &meta, text)?;
            }
        }

        Ok(())
    }

    async fn websocket_close(
        &self,
        ws: WebSocket,
        _code: usize,
        _reason: String,
        _was_clean: bool,
    ) -> Result<()> {
        if let Ok(meta) = self.get_connection_meta(&ws) {
            let peer_count = self.state.get_websockets().len().saturating_sub(1);
            let msg = ServerControlMessage::PeerLeft {
                guest_id: meta.user_id,
                peer_count,
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                self.broadcast_text(&json, Some(&ws));
            }
        }

        ws.close(Some(1000), Some("goodbye"))?;
        Ok(())
    }

    async fn websocket_error(&self, ws: WebSocket, error: Error) -> Result<()> {
        console_log!("WebSocket error: {:?}", error);
        ws.close(Some(1011), Some("error"))?;
        Ok(())
    }
}

impl NamespaceSyncDO {
    fn ensure_schema(&self) -> Result<()> {
        let sql = self.state.storage().sql();
        let storage = DoSyncStorage::new(sql);
        storage
            .init_schema()
            .map_err(|e| Error::from(format!("Schema init: {e}")))
    }

    fn doc_manager(&self) -> Result<SyncDocManager> {
        let sql = self.state.storage().sql();
        let storage = DoSyncStorage::new(sql);
        Ok(SyncDocManager::new(Arc::new(storage)))
    }

    fn get_connection_meta(&self, ws: &WebSocket) -> Result<ConnectionMeta> {
        ws.deserialize_attachment::<ConnectionMeta>()
            .map_err(|e| Error::from(format!("Deserialize attachment: {e}")))?
            .ok_or_else(|| Error::from("No attachment on WebSocket"))
    }

    fn broadcast_text(&self, message: &str, _exclude: Option<&WebSocket>) {
        for ws in self.state.get_websockets() {
            let _ = ws.send_with_str(message);
        }
    }

    fn broadcast_binary(&self, data: &[u8], _exclude: Option<&WebSocket>) {
        for ws in self.state.get_websockets() {
            let _ = ws.send_with_bytes(data);
        }
    }

    async fn handle_websocket_upgrade(&self, req: Request) -> Result<Response> {
        self.ensure_schema()?;

        // Extract auth info from query params (set by the worker handler)
        let url = req.url()?;
        let params: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let user_id = params
            .get("user_id")
            .ok_or_else(|| Error::from("Missing user_id"))?
            .clone();
        let workspace_id = params
            .get("workspace_id")
            .ok_or_else(|| Error::from("Missing workspace_id"))?
            .clone();
        let device_id = params.get("device_id").cloned();
        let is_guest = params.get("is_guest").map(|v| v == "true").unwrap_or(false);
        let read_only = params
            .get("read_only")
            .map(|v| v == "true")
            .unwrap_or(false);

        // Create WebSocket pair
        let WebSocketPair { client, server } = WebSocketPair::new()?;

        // Accept with hibernation
        self.state.accept_web_socket(&server);

        // Attach auth metadata for hibernation persistence
        let meta = ConnectionMeta {
            user_id: user_id.clone(),
            workspace_id: workspace_id.clone(),
            device_id,
            is_guest,
            read_only,
            handshake_complete: false,
        };
        server
            .serialize_attachment(&meta)
            .map_err(|e| Error::from(format!("Serialize attachment: {e}")))?;

        // Send file manifest
        let manager = self.doc_manager()?;
        let manifest_entries = manager
            .generate_file_manifest(&workspace_id)
            .map_err(|e| Error::from(format!("Manifest: {e}")))?;

        let manifest = ServerControlMessage::FileManifest {
            files: manifest_entries,
            client_is_new: false,
        };
        let manifest_json =
            serde_json::to_string(&manifest).map_err(|e| Error::from(e.to_string()))?;
        server.send_with_str(&manifest_json)?;

        // Broadcast peer_joined
        let peer_count = self.state.get_websockets().len() + 1;
        let peer_msg = ServerControlMessage::PeerJoined {
            guest_id: user_id,
            peer_count,
        };
        if let Ok(json) = serde_json::to_string(&peer_msg) {
            self.broadcast_text(&json, Some(&server));
        }

        Response::from_websocket(client)
    }

    fn handle_binary_message(
        &self,
        ws: &WebSocket,
        meta: &ConnectionMeta,
        data: Vec<u8>,
    ) -> Result<()> {
        if meta.read_only {
            return Ok(());
        }

        // Parse doc_id from v2 framing: [u8: doc_id_len][bytes: doc_id][bytes: y-sync payload]
        if data.is_empty() {
            return Ok(());
        }

        let doc_id_len = data[0] as usize;
        if data.len() < 1 + doc_id_len {
            return Ok(());
        }

        let doc_id = std::str::from_utf8(&data[1..1 + doc_id_len])
            .map_err(|e| Error::from(format!("Invalid doc_id: {e}")))?;
        let payload = &data[1 + doc_id_len..];

        let doc_type = DocType::parse(doc_id)
            .ok_or_else(|| Error::from(format!("Invalid doc_id: {}", doc_id)))?;

        let storage_key = doc_type.storage_key();

        // Persist the update
        let manager = self.doc_manager()?;
        manager
            .apply_change(
                &storage_key,
                payload,
                UpdateOrigin::Remote,
                meta.device_id.as_deref(),
                None,
            )
            .map_err(|e| Error::from(format!("Apply change: {e}")))?;

        // Broadcast to all connections (including sender — client will deduplicate)
        self.broadcast_binary(&data, Some(ws));

        Ok(())
    }

    fn handle_text_message(
        &self,
        ws: &WebSocket,
        meta: &ConnectionMeta,
        text: String,
    ) -> Result<()> {
        let json: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        match json.get("type").and_then(|v| v.as_str()) {
            Some("files_ready") | Some("FilesReady") => {
                let manager = self.doc_manager()?;
                let messages = manager
                    .complete_handshake(&meta.workspace_id)
                    .map_err(|e| Error::from(format!("Handshake: {e}")))?;

                for msg in messages {
                    ws.send_with_str(&msg)?;
                }

                // Update meta
                let mut updated = meta.clone();
                updated.handshake_complete = true;
                ws.serialize_attachment(&updated)
                    .map_err(|e| Error::from(format!("Serialize: {e}")))?;
            }
            Some("focus") | Some("unfocus") => {
                self.broadcast_text(&text, Some(ws));
            }
            _ => {}
        }

        Ok(())
    }
}
