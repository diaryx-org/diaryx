use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::{Duration, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::host_fns::WebSocketBridge;

pub trait SyncGuestBridge: Send + Sync {
    fn call_binary_export(&self, export_name: &str, input: &[u8]) -> Result<(), String>;
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsRequest {
    Connect {
        server_url: String,
        workspace_id: String,
        auth_token: Option<String>,
        session_code: Option<String>,
        write_to_disk: Option<bool>,
    },
    SendBinary {
        data: String,
    },
    SendText {
        text: String,
    },
    Disconnect,
}

enum TransportCommand {
    SendBinary(Vec<u8>),
    SendText(String),
    Disconnect,
}

struct BridgeState {
    connection_key: Option<String>,
    sender: Option<UnboundedSender<TransportCommand>>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            connection_key: None,
            sender: None,
        }
    }
}

pub struct TokioWebSocketBridge {
    guest: Arc<Mutex<Option<Weak<dyn SyncGuestBridge>>>>,
    state: Mutex<BridgeState>,
}

impl TokioWebSocketBridge {
    pub fn new() -> Self {
        Self {
            guest: Arc::new(Mutex::new(None)),
            state: Mutex::new(BridgeState::new()),
        }
    }

    pub fn set_guest_bridge(&self, guest: Weak<dyn SyncGuestBridge>) {
        if let Ok(mut guard) = self.guest.lock() {
            *guard = Some(guest);
        }
    }

    fn normalize_server_base(server_url: &str) -> String {
        let mut base = server_url.trim().trim_end_matches('/').to_string();
        loop {
            if let Some(stripped) = base.strip_suffix("/sync2") {
                base = stripped.trim_end_matches('/').to_string();
                continue;
            }
            if let Some(stripped) = base.strip_suffix("/sync") {
                base = stripped.trim_end_matches('/').to_string();
                continue;
            }
            break;
        }
        base
    }

    fn connection_key(
        server_url: &str,
        workspace_id: &str,
        auth_token: Option<&str>,
        session_code: Option<&str>,
        write_to_disk: Option<bool>,
    ) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            server_url.trim(),
            workspace_id.trim(),
            auth_token.unwrap_or(""),
            session_code.unwrap_or(""),
            if write_to_disk == Some(false) { 0 } else { 1 }
        )
    }

    fn send_command(&self, command: TransportCommand) -> Result<(), String> {
        let sender = {
            let guard = self
                .state
                .lock()
                .map_err(|e| format!("Failed to lock websocket bridge state: {e}"))?;
            guard
                .sender
                .clone()
                .ok_or_else(|| "WebSocket bridge is not connected".to_string())?
        };
        sender
            .send(command)
            .map_err(|_| "WebSocket bridge channel is closed".to_string())
    }

    fn invoke_guest(
        guest: &Arc<Mutex<Option<Weak<dyn SyncGuestBridge>>>>,
        export_name: &str,
        input: &[u8],
    ) -> Result<(), String> {
        let callback = guest
            .lock()
            .map_err(|e| format!("Failed to lock guest callback: {e}"))?
            .as_ref()
            .cloned()
            .ok_or_else(|| "Sync guest callback is unavailable".to_string())?
            .upgrade()
            .ok_or_else(|| "Sync guest callback is unavailable".to_string())?;
        callback.call_binary_export(export_name, input)
    }

    fn spawn_worker(
        &self,
        server_url: String,
        workspace_id: String,
        auth_token: Option<String>,
        session_code: Option<String>,
        write_to_disk: Option<bool>,
        receiver: UnboundedReceiver<TransportCommand>,
    ) {
        let guest = Arc::clone(&self.guest);

        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::warn!("Failed to create websocket bridge runtime: {e}");
                    return;
                }
            };

            runtime.block_on(async move {
                run_bridge_loop(
                    guest,
                    receiver,
                    server_url,
                    workspace_id,
                    auth_token,
                    session_code,
                    write_to_disk,
                )
                .await;
            });
        });
    }
}

impl Default for TokioWebSocketBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketBridge for TokioWebSocketBridge {
    fn request(&self, request_json: &str) -> Result<String, String> {
        let request: WsRequest =
            serde_json::from_str(request_json).map_err(|e| format!("Invalid ws request: {e}"))?;

        match request {
            WsRequest::Connect {
                server_url,
                workspace_id,
                auth_token,
                session_code,
                write_to_disk,
            } => {
                let next_key = Self::connection_key(
                    &server_url,
                    &workspace_id,
                    auth_token.as_deref(),
                    session_code.as_deref(),
                    write_to_disk,
                );

                {
                    let guard = self
                        .state
                        .lock()
                        .map_err(|e| format!("Failed to lock websocket bridge state: {e}"))?;
                    if guard.connection_key.as_deref() == Some(next_key.as_str())
                        && guard.sender.is_some()
                    {
                        return Ok(r#"{"ok":true}"#.to_string());
                    }
                }

                let _ = self.send_command(TransportCommand::Disconnect);

                let (sender, receiver) = unbounded_channel();
                {
                    let mut guard = self
                        .state
                        .lock()
                        .map_err(|e| format!("Failed to lock websocket bridge state: {e}"))?;
                    guard.connection_key = Some(next_key);
                    guard.sender = Some(sender);
                }

                self.spawn_worker(
                    server_url,
                    workspace_id,
                    auth_token,
                    session_code,
                    write_to_disk,
                    receiver,
                );

                Ok(r#"{"ok":true}"#.to_string())
            }
            WsRequest::SendBinary { data } => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|e| format!("Invalid websocket payload: {e}"))?;
                self.send_command(TransportCommand::SendBinary(bytes))?;
                Ok(r#"{"ok":true}"#.to_string())
            }
            WsRequest::SendText { text } => {
                self.send_command(TransportCommand::SendText(text))?;
                Ok(r#"{"ok":true}"#.to_string())
            }
            WsRequest::Disconnect => {
                let _ = self.send_command(TransportCommand::Disconnect);
                let mut guard = self
                    .state
                    .lock()
                    .map_err(|e| format!("Failed to lock websocket bridge state: {e}"))?;
                guard.connection_key = None;
                guard.sender = None;
                Ok(r#"{"ok":true}"#.to_string())
            }
        }
    }
}

async fn run_bridge_loop(
    guest: Arc<Mutex<Option<Weak<dyn SyncGuestBridge>>>>,
    mut receiver: UnboundedReceiver<TransportCommand>,
    server_url: String,
    workspace_id: String,
    auth_token: Option<String>,
    session_code: Option<String>,
    write_to_disk: Option<bool>,
) {
    let mut queued_commands = VecDeque::new();
    let mut reconnect_attempt = 0u32;
    let mut should_reconnect = true;

    loop {
        if !should_reconnect {
            return;
        }

        let ws_url = match build_ws_url(
            &server_url,
            &workspace_id,
            auth_token.as_deref(),
            session_code.as_deref(),
        ) {
            Ok(url) => url,
            Err(e) => {
                log::warn!("Invalid sync websocket URL: {e}");
                return;
            }
        };

        match connect_async(ws_url.as_str()).await {
            Ok((ws_stream, _response)) => {
                reconnect_attempt = 0;
                let connected_payload = serde_json::json!({
                    "workspace_id": workspace_id,
                    "write_to_disk": write_to_disk.unwrap_or(true),
                })
                .to_string();
                if let Err(e) = TokioWebSocketBridge::invoke_guest(
                    &guest,
                    "on_connected",
                    connected_payload.as_bytes(),
                ) {
                    log::warn!("Failed to notify guest of websocket connect: {e}");
                }

                let (mut write, mut read) = ws_stream.split();
                let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
                ping_interval.tick().await;

                'connection: loop {
                    while let Some(command) = queued_commands.pop_front() {
                        if !handle_outgoing_command(&mut write, command).await {
                            should_reconnect = false;
                            break 'connection;
                        }
                    }

                    tokio::select! {
                        maybe_command = receiver.recv() => {
                            match maybe_command {
                                Some(TransportCommand::SendBinary(bytes)) => {
                                    if write.send(Message::Binary(bytes.into())).await.is_err() {
                                        break 'connection;
                                    }
                                }
                                Some(TransportCommand::SendText(text)) => {
                                    if write.send(Message::Text(text.into())).await.is_err() {
                                        break 'connection;
                                    }
                                }
                                Some(TransportCommand::Disconnect) | None => {
                                    should_reconnect = false;
                                    let _ = write.send(Message::Close(None)).await;
                                    break 'connection;
                                }
                            }
                        }
                        message = read.next() => {
                            match message {
                                Some(Ok(Message::Binary(data))) => {
                                    if let Err(e) = TokioWebSocketBridge::invoke_guest(&guest, "handle_binary_message", &data) {
                                        log::warn!("Failed to forward binary sync frame to guest: {e}");
                                        should_reconnect = false;
                                        break 'connection;
                                    }
                                }
                                Some(Ok(Message::Text(text))) => {
                                    if let Err(e) = TokioWebSocketBridge::invoke_guest(&guest, "handle_text_message", text.as_bytes()) {
                                        log::warn!("Failed to forward text sync frame to guest: {e}");
                                        should_reconnect = false;
                                        break 'connection;
                                    }
                                }
                                Some(Ok(Message::Ping(data))) => {
                                    if write.send(Message::Pong(data)).await.is_err() {
                                        break 'connection;
                                    }
                                }
                                Some(Ok(Message::Close(_))) | None => {
                                    break 'connection;
                                }
                                Some(Err(e)) => {
                                    log::warn!("Sync websocket error: {e}");
                                    break 'connection;
                                }
                                _ => {}
                            }
                        }
                        _ = ping_interval.tick() => {
                            if write.send(Message::Ping(Vec::new().into())).await.is_err() {
                                break 'connection;
                            }
                        }
                    }
                }

                if let Err(e) = TokioWebSocketBridge::invoke_guest(&guest, "on_disconnected", &[]) {
                    log::warn!("Failed to notify guest of websocket disconnect: {e}");
                    should_reconnect = false;
                }
            }
            Err(e) => {
                log::warn!("Failed to connect sync websocket: {e}");
                if let Err(callback_error) =
                    TokioWebSocketBridge::invoke_guest(&guest, "on_disconnected", &[])
                {
                    log::warn!("Failed to notify guest of websocket disconnect: {callback_error}");
                    return;
                }
            }
        }

        if !should_reconnect {
            return;
        }

        reconnect_attempt += 1;
        let delay_ms =
            ((1000f64) * 1.5f64.powi((reconnect_attempt.saturating_sub(1)) as i32)).round() as u64;
        let delay_ms = delay_ms.min(30_000);

        tokio::select! {
            _ = sleep(Duration::from_millis(delay_ms)) => {}
            maybe_command = receiver.recv() => {
                match maybe_command {
                    Some(TransportCommand::Disconnect) | None => return,
                    Some(command) => queued_commands.push_back(command),
                }
            }
        }
    }
}

async fn handle_outgoing_command<S>(write: &mut S, command: TransportCommand) -> bool
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match command {
        TransportCommand::SendBinary(bytes) => {
            write.send(Message::Binary(bytes.into())).await.is_ok()
        }
        TransportCommand::SendText(text) => write.send(Message::Text(text.into())).await.is_ok(),
        TransportCommand::Disconnect => {
            let _ = write.send(Message::Close(None)).await;
            false
        }
    }
}

fn build_ws_url(
    server_url: &str,
    workspace_id: &str,
    auth_token: Option<&str>,
    session_code: Option<&str>,
) -> Result<Url, String> {
    let base = TokioWebSocketBridge::normalize_server_base(server_url);
    let mut url = Url::parse(&format!("{}/sync2", base)).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("workspace_id", workspace_id);
    if let Some(token) = auth_token {
        url.query_pairs_mut().append_pair("token", token);
    }
    if let Some(code) = session_code {
        url.query_pairs_mut().append_pair("session", code);
    }
    Ok(url)
}
