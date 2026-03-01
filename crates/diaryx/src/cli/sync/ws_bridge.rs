//! WebSocket bridge — connects tokio-tungstenite to Extism sync plugin.
//!
//! The Rust equivalent of the browser's `pluginSyncAdapter.ts`.
//! Owns the WebSocket connection and bridges frames to/from the sync plugin
//! via the binary action protocol.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use diaryx_extism::ExtismPluginAdapter;
use diaryx_extism::binary_protocol::{DecodedAction, decode_actions};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type WsSink = futures_util::stream::SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

/// Configuration for the WebSocket bridge.
pub struct WsBridgeConfig {
    /// WebSocket URL (e.g. `wss://sync.diaryx.org/sync2`)
    pub ws_url: String,
    /// Auth token for the connection
    pub auth_token: Option<String>,
    /// Workspace ID
    pub workspace_id: String,
}

/// Bridge between a tokio-tungstenite WebSocket and the Extism sync plugin.
pub struct WsBridge {
    config: WsBridgeConfig,
    plugin: Arc<Mutex<ExtismPluginAdapter>>,
}

impl WsBridge {
    pub fn new(config: WsBridgeConfig, plugin: ExtismPluginAdapter) -> Self {
        Self {
            config,
            plugin: Arc::new(Mutex::new(plugin)),
        }
    }

    /// Run the WebSocket bridge until shutdown is signaled.
    pub async fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        // Build WS URL with auth params
        let mut url = self.config.ws_url.clone();
        if url.contains('?') {
            url.push('&');
        } else {
            url.push('?');
        }
        url.push_str(&format!("workspace_id={}", self.config.workspace_id));
        if let Some(ref token) = self.config.auth_token {
            url.push_str(&format!("&token={}", token));
        }

        let (ws_stream, _response) = connect_async(&url)
            .await
            .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // Notify plugin of connection
        let connect_config = serde_json::json!({
            "workspace_id": self.config.workspace_id,
        });

        let actions = {
            let plugin = self.plugin.lock().await;
            plugin
                .call_guest_binary("on_connected", connect_config.to_string().as_bytes())
                .map_err(|e| format!("on_connected failed: {}", e))?
        };

        // Process actions from on_connected
        Self::process_actions(&actions, &mut write).await?;

        // Main loop: receive WS frames and forward to plugin
        while running.load(Ordering::Relaxed) {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Binary(data))) => {
                            let actions = {
                                let plugin = self.plugin.lock().await;
                                plugin
                                    .call_guest_binary("handle_binary_message", &data)
                                    .unwrap_or_default()
                            };
                            if let Err(e) = Self::process_actions(&actions, &mut write).await {
                                eprintln!("  Error processing binary message: {}", e);
                            }
                        }
                        Some(Ok(Message::Text(text))) => {
                            let actions = {
                                let plugin = self.plugin.lock().await;
                                plugin
                                    .call_guest_binary("handle_text_message", text.as_bytes())
                                    .unwrap_or_default()
                            };
                            if let Err(e) = Self::process_actions(&actions, &mut write).await {
                                eprintln!("  Error processing text message: {}", e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            println!("  WebSocket closed by server");
                            break;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Err(e)) => {
                            eprintln!("  WebSocket error: {}", e);
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nShutting down sync...");
                    break;
                }
            }
        }

        // Notify plugin of disconnection
        {
            let plugin = self.plugin.lock().await;
            let _ = plugin.call_guest_binary("on_disconnected", &[]);
        }

        Ok(())
    }

    /// Process decoded actions from plugin response — send frames, emit events.
    async fn process_actions(action_bytes: &[u8], write: &mut WsSink) -> Result<(), String> {
        if action_bytes.is_empty() {
            return Ok(());
        }

        let actions = decode_actions(action_bytes)?;

        for action in actions {
            match action {
                DecodedAction::SendBinary(data) => {
                    write
                        .send(Message::Binary(data.into()))
                        .await
                        .map_err(|e| format!("Failed to send binary: {}", e))?;
                }
                DecodedAction::SendText(text) => {
                    write
                        .send(Message::Text(text.into()))
                        .await
                        .map_err(|e| format!("Failed to send text: {}", e))?;
                }
                DecodedAction::EmitEvent(event_bytes) => {
                    if let Ok(event_str) = String::from_utf8(event_bytes) {
                        log::debug!("Plugin event: {}", event_str);
                    }
                }
                DecodedAction::DownloadSnapshot(snapshot_id) => {
                    log::debug!("Snapshot download requested: {}", snapshot_id);
                }
            }
        }

        Ok(())
    }
}
