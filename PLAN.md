# Plan: Apple URLSession-based SyncTransport Implementation

## Architecture

Swift owns the WebSocket via `URLSessionWebSocketTask` (same ownership model
as WASM, where JavaScript owns the WebSocket). Rust handles all protocol logic
via `SyncClient<FS, AppleConnector>`.

```
┌──────────────────────────────────────────────────────────────┐
│ Swift                                                        │
│                                                              │
│  AppleWebSocketHandler (implements AppleWebSocketDelegate)   │
│    ├── openWebsocket(url) → URLSessionWebSocketTask          │
│    ├── sendBinary(data) → task.send(.data(data))             │
│    ├── sendText(text) → task.send(.string(text))             │
│    ├── sendPing() → task.sendPing()                          │
│    └── closeWebsocket() → task.cancel()                      │
│                                                              │
│  Receive loop (Swift async):                                 │
│    task.receive { msg in                                     │
│      bridge.onBinaryMessage(data)  // or onTextMessage(text) │
│    }                                                         │
│                                                              │
│  Lifecycle:                                                  │
│    bridge.onConnected()     // after WS handshake            │
│    bridge.onDisconnected()  // on close/error                │
│    bridge.onError(message)  // on failure                    │
│                                                              │
└────────────────────────┬─────────────────────────────────────┘
                         │ UniFFI
┌────────────────────────▼─────────────────────────────────────┐
│ Rust (diaryx_apple/src/sync.rs)                              │
│                                                              │
│  AppleConnector: TransportConnector                          │
│    connect(url) → stores channels, calls delegate.open(),    │
│                   awaits on_connected signal                 │
│                                                              │
│  AppleTransport: SyncTransport                               │
│    send_binary/text/ping → delegate.send_*() (callback)      │
│    recv() → incoming_rx.recv() (fed by on_*_message)         │
│    close() → delegate.close_websocket()                      │
│                                                              │
│  AppleSyncBridge: UniFFI Object (exposed to Swift)           │
│    on_connected() → signals oneshot channel                  │
│    on_binary_message(data) → pushes to incoming channel      │
│    on_text_message(text) → pushes to incoming channel        │
│    on_disconnected() → closes incoming channel               │
│    on_error(message) → pushes error to incoming channel      │
│                                                              │
│  SyncClient<FS, AppleConnector> runs on tokio runtime        │
└──────────────────────────────────────────────────────────────┘
```

## Step 1: Split `native-sync` feature in diaryx_core

Currently `native-sync` bundles everything:
```toml
native-sync = ["crdt", "dep:tokio", "dep:tokio-tungstenite", "dep:url", ...]
```

Split into two features:
- `sync-client` = traits + SyncClient (no transport impl)
- `native-sync` = `sync-client` + TokioTransport (tokio-tungstenite)

This lets `diaryx_apple` depend on `sync-client` without pulling in
tokio-tungstenite (it provides its own transport via URLSession).

**File: `crates/diaryx_core/Cargo.toml`**

```toml
# Base sync client: traits (SyncTransport, TransportConnector) + SyncClient
# Available on non-WASM targets. Does NOT include any transport implementation.
sync-client = ["crdt", "dep:tokio", "dep:futures-util", "dep:async-trait"]

# Adds TokioTransport/TokioConnector using tokio-tungstenite (CLI, Tauri desktop)
native-sync = ["sync-client", "dep:tokio-tungstenite", "dep:url"]
```

**File: `crates/diaryx_core/src/crdt/mod.rs`**

Change cfg gates:
- `transport.rs`, `sync_client.rs` → gated on `sync-client`
- `tokio_transport.rs` → stays gated on `native-sync`

```rust
// Sync client (traits + SyncClient) — available with sync-client feature
#[cfg(all(not(target_arch = "wasm32"), feature = "sync-client"))]
mod sync_client;
#[cfg(all(not(target_arch = "wasm32"), feature = "sync-client"))]
mod transport;

// TokioTransport — only with native-sync (includes tokio-tungstenite)
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
mod tokio_transport;
```

Re-exports similarly split:
```rust
#[cfg(all(not(target_arch = "wasm32"), feature = "sync-client"))]
pub use sync_client::{...};
#[cfg(all(not(target_arch = "wasm32"), feature = "sync-client"))]
pub use transport::{...};

#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use tokio_transport::{TokioConnector, TokioTransport};
```

**Verify**: CLI and Tauri still use `native-sync` → no changes to their call sites.

---

## Step 2: Add dependencies to diaryx_apple

**File: `crates/diaryx_apple/Cargo.toml`**

```toml
[dependencies]
diaryx_core = { workspace = true, features = ["sync-client"] }
tokio = { version = "1", features = ["sync", "rt-multi-thread"] }
log = "0.4"
```

Note: `sync-client` gives us the traits and SyncClient without
tokio-tungstenite. tokio is needed for channels and the runtime.

---

## Step 3: Create `diaryx_apple/src/sync.rs`

### 3a. UniFFI callback interface (Swift implements)

```rust
/// Callback interface that Swift implements to drive the WebSocket connection.
///
/// Methods are called from Rust's tokio runtime thread. Swift implementations
/// should dispatch async work (like URLSessionWebSocketTask.send) appropriately.
#[uniffi::export(callback_interface)]
pub trait AppleWebSocketDelegate: Send + Sync {
    /// Open a WebSocket connection to the given URL.
    fn open_websocket(&self, url: String);
    /// Send a binary WebSocket frame.
    fn send_binary(&self, data: Vec<u8>);
    /// Send a text WebSocket frame.
    fn send_text(&self, text: String);
    /// Send a WebSocket ping frame.
    fn send_ping(&self);
    /// Close the WebSocket connection.
    fn close_websocket(&self);
}
```

### 3b. AppleTransport

```rust
pub(crate) struct AppleTransport {
    delegate: Arc<dyn AppleWebSocketDelegate>,
    incoming_rx: tokio::sync::mpsc::UnboundedReceiver<Result<WsMessage, TransportError>>,
}
```

Implements `SyncTransport`:
- `send_binary` → `delegate.send_binary(data)` (fire-and-forget via callback)
- `send_text` → `delegate.send_text(text)`
- `send_ping` → `delegate.send_ping()`
- `recv()` → `incoming_rx.recv().await`
- `close()` → `delegate.close_websocket()`

### 3c. AppleConnector

```rust
pub(crate) struct AppleConnector {
    delegate: Arc<dyn AppleWebSocketDelegate>,
    state: Arc<std::sync::Mutex<ConnectionState>>,
}

struct ConnectionState {
    incoming_tx: Option<tokio::sync::mpsc::UnboundedSender<Result<WsMessage, TransportError>>>,
    connected_tx: Option<tokio::sync::oneshot::Sender<Result<(), TransportError>>>,
}
```

Implements `TransportConnector`:
- `connect(url)`:
  1. Create `mpsc::unbounded_channel` for incoming messages
  2. Create `oneshot::channel` for connection signal
  3. Store senders in `state`
  4. Call `delegate.open_websocket(url)`
  5. Await `connected_rx`
  6. Return `AppleTransport { delegate, incoming_rx }`

### 3d. AppleSyncBridge (exposed to Swift via UniFFI)

```rust
#[derive(uniffi::Object)]
pub struct AppleSyncBridge {
    state: Arc<std::sync::Mutex<ConnectionState>>,
}
```

Methods exposed to Swift:
- `on_connected()` → takes `connected_tx`, sends `Ok(())`
- `on_binary_message(data: Vec<u8>)` → sends `Ok(WsMessage::Binary(data))` to `incoming_tx`
- `on_text_message(text: String)` → sends `Ok(WsMessage::Text(text))` to `incoming_tx`
- `on_disconnected()` → drops `incoming_tx` (causes `recv()` to return `None`)
- `on_error(message: String)` → sends `Err(TransportError)` then drops `incoming_tx`

---

## Step 4: Create Swift implementation

**File: `apps/apple/Diaryx/Sync/AppleWebSocketHandler.swift`**

```swift
import Foundation

class AppleWebSocketHandler: AppleWebSocketDelegate {
    private var task: URLSessionWebSocketTask?
    private weak var bridge: AppleSyncBridge?
    private let session: URLSession

    init(bridge: AppleSyncBridge) {
        self.bridge = bridge
        self.session = URLSession(configuration: .default)
    }

    func openWebsocket(url: String) {
        guard let wsUrl = URL(string: url) else { ... }
        task = session.webSocketTask(with: wsUrl)
        task?.resume()
        bridge?.onConnected()
        startReceiveLoop()
    }

    func sendBinary(data: Data) {
        task?.send(.data(data)) { [weak self] error in
            if let error { self?.bridge?.onError(message: error.localizedDescription) }
        }
    }

    func sendText(text: String) {
        task?.send(.string(text)) { [weak self] error in
            if let error { self?.bridge?.onError(message: error.localizedDescription) }
        }
    }

    func sendPing() {
        task?.sendPing { [weak self] error in
            if let error { self?.bridge?.onError(message: error.localizedDescription) }
        }
    }

    func closeWebsocket() {
        task?.cancel(with: .normalClosure, reason: nil)
    }

    private func startReceiveLoop() {
        task?.receive { [weak self] result in
            switch result {
            case .success(.data(let data)):
                self?.bridge?.onBinaryMessage(data: data)
                self?.startReceiveLoop()
            case .success(.string(let text)):
                self?.bridge?.onTextMessage(text: text)
                self?.startReceiveLoop()
            case .failure(let error):
                self?.bridge?.onError(message: error.localizedDescription)
                self?.bridge?.onDisconnected()
            @unknown default:
                break
            }
        }
    }
}
```

---

## Step 5: Update docs

- `crates/diaryx_apple/README.md` — document sync module, callback interface
- `crates/diaryx_core/src/crdt/README.md` — mention Apple transport
- `crates/diaryx_core/Cargo.toml` — update feature docs

---

## Step 6: Verification

1. `cargo check -p diaryx_core --features sync-client` — traits compile without tungstenite
2. `cargo check -p diaryx_core --features native-sync` — still works (superset)
3. `cargo check -p diaryx` — CLI still compiles
4. `cargo check -p diaryx_apple` — Apple crate compiles with new sync module
5. `cargo test -p diaryx_core` — tests pass
6. `cargo test -p diaryx_apple` — tests pass

---

## What This Does NOT Include (future work)

- CRDT initialization in the Apple app (WorkspaceCrdt, BodyDocManager, SqliteStorage)
- Authentication flow (magic links, session tokens)
- Sync UI in the Apple app (status indicators, connect/disconnect)
- Background sync / lifecycle handling
- Integration into WorkspaceState
