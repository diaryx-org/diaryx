# Plan: Unify tokio-tungstenite, Fix TLS, Make SyncClient Generic

## Key Finding: tauri-plugin-websocket Cannot Implement SyncTransport

`tauri-plugin-websocket` **uses `tokio-tungstenite` internally** (v0.28) — it does NOT use
platform-native networking on iOS. Its source (`plugins/websocket/src/lib.rs`) shows:

```rust
use tokio_tungstenite::{connect_async_tls_with_config, ...};
// WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>
```

Furthermore, it exposes **no public Rust API**. Its `connect` and `send` functions are
`#[tauri::command]` IPC handlers callable only from frontend JavaScript. The internal types
(`ConnectionManager`, `WebSocketWriter`) are all `pub(crate)`. There is no `ios/` directory;
no native Swift/Kotlin networking code exists.

**Consequence**: Using tauri-plugin-websocket as a SyncTransport would mean adding a dependency
that uses the exact same networking stack (tokio-tungstenite) we already have, provides no iOS
networking benefits, and has no usable Rust API. It should not be part of this plan.

**What to do instead for Tauri iOS (future)**: Write a custom `SyncTransport` implementation
backed by Apple's `URLSessionWebSocketTask` via Swift FFI. That's a separate effort. Making
`SyncClient` generic now sets up the architecture for it.

---

## Current State

Three copies of `tokio-tungstenite` in the dependency tree:

| Crate | Version | TLS Features | Actually Used? |
|-------|---------|-------------|----------------|
| `diaryx_core` | 0.26 | **none** | YES — `TokioTransport` |
| `diaryx` (CLI) | 0.24 | `rustls-tls-native-roots` | **NO** — dead leftover |
| `diaryx_tauri` | 0.23 | none | **NO** — dead leftover |

Both CLI and Tauri delegate all WebSocket work to `diaryx_core::crdt::SyncClient` via the
`native-sync` feature. Neither crate imports `tokio_tungstenite` in its own code.

---

## Step 1: Remove dead tokio-tungstenite dependencies

**Files to change:**

- `crates/diaryx/Cargo.toml` — Remove line 32: `tokio-tungstenite = { version = "0.24", ... }`
- `apps/tauri/src-tauri/Cargo.toml` — Remove line 50: `tokio-tungstenite = "0.23"` and
  the `# Live sync (WebSocket transport)` comment block. Also remove `futures-util = "0.3"`
  and `url = "2"` from lines 51-52 if they're only used for the old direct websocket code
  (verify with grep first).

**Verification**: `cargo check -p diaryx` and `cargo check -p diaryx_tauri` should both
compile without the removed dependencies.

---

## Step 2: Add TLS to diaryx_core's tokio-tungstenite

**File**: `crates/diaryx_core/Cargo.toml`

Change line 55 from:
```toml
tokio-tungstenite = { version = "0.26", optional = true }
```
to:
```toml
tokio-tungstenite = { version = "0.26", optional = true, features = ["rustls-tls-webpki-roots"] }
```

**Why `rustls-tls-webpki-roots`**:
- `rustls` is pure Rust — cross-compiles cleanly to iOS, Android, all desktop targets
- `webpki-roots` bundles Mozilla's root certificates, avoiding platform-specific root store access
  (important: iOS doesn't expose system roots to Rust the way desktop OSes do)
- `native-roots` would use `rustls-native-certs` which works on desktop but may have issues on iOS
- The CLI already uses rustls (on its own 0.24 dep), so this aligns with existing precedent

**Verification**: The `TokioTransport::connect("wss://...")` call should now succeed where it
previously would have failed with a "TLS not enabled" error.

---

## Step 3: Add `TransportConnector` trait to `transport.rs`

**File**: `crates/diaryx_core/src/crdt/transport.rs`

Add a new trait below `SyncTransport`:

```rust
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
```

**File**: `crates/diaryx_core/src/crdt/tokio_transport.rs`

Add `TransportConnector` impl for a new `TokioConnector` zero-size struct:

```rust
/// Connector that creates `TokioTransport` connections via `tokio-tungstenite`.
pub struct TokioConnector;

#[async_trait::async_trait]
impl TransportConnector for TokioConnector {
    type Transport = TokioTransport;

    async fn connect(&self, url: &str) -> Result<Self::Transport, TransportError> {
        TokioTransport::connect(url).await
    }
}
```

---

## Step 4: Make `SyncClient` generic over `TransportConnector`

**File**: `crates/diaryx_core/src/crdt/sync_client.rs`

### 4a. Add connector type parameter to struct

```rust
pub struct SyncClient<FS: AsyncFileSystem, C: TransportConnector> {
    config: SyncClientConfig,
    sync_manager: Arc<RustSyncManager<FS>>,
    handler: Arc<dyn SyncEventHandler>,
    session: SyncSession<FS>,
    connector: C,
}
```

### 4b. Update `new()` to accept a connector

```rust
pub fn new(
    config: SyncClientConfig,
    sync_manager: Arc<RustSyncManager<FS>>,
    handler: Arc<dyn SyncEventHandler>,
    connector: C,
) -> Self { ... }
```

### 4c. Replace `TokioTransport` in private methods with `C::Transport`

- `execute_actions`: `transport: &mut C::Transport`
- `run_session`: `transport: &mut C::Transport`

### 4d. Replace `TokioTransport::connect()` calls with `self.connector.connect()`

In `run_persistent` (line 250):
```rust
let mut transport = match self.connector.connect(&ws_url).await {
```

In `run_one_shot` (line 306):
```rust
let mut transport = self.connector.connect(&ws_url).await?;
```

### 4e. Remove the direct `TokioTransport` import from sync_client.rs

Line 41 (`use super::tokio_transport::TokioTransport;`) can be removed.

---

## Step 5: Add convenience type alias and update re-exports

**File**: `crates/diaryx_core/src/crdt/mod.rs`

Add re-exports for the new types:

```rust
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use tokio_transport::{TokioConnector, TokioTransport};
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub use transport::TransportConnector;
```

Optionally add a type alias for ergonomics:

```rust
#[cfg(all(not(target_arch = "wasm32"), feature = "native-sync"))]
pub type NativeSyncClient<FS> = SyncClient<FS, TokioConnector>;
```

---

## Step 6: Update call sites (CLI and Tauri)

### CLI (`crates/diaryx/src/cli/sync/client.rs`)

Change:
```rust
let client = SyncClient::new(client_config, sync_manager, Arc::new(handler));
```
to:
```rust
let client = SyncClient::new(client_config, sync_manager, Arc::new(handler), TokioConnector);
```

Add import: `use diaryx_core::crdt::TokioConnector;`

### Tauri (`apps/tauri/src-tauri/src/commands.rs`)

Same pattern — pass `TokioConnector` to `SyncClient::new()`.

---

## Step 7: Update docs

- `crates/diaryx_core/src/crdt/README.md` — note TransportConnector trait and how to add
  custom transports
- `apps/tauri/README.md` — note the architecture allows swapping transports for iOS

---

## WASM: No Changes Needed

The WASM sync client (`crates/diaryx_wasm/src/wasm_sync_client.rs`) uses an inherently
different pattern — JavaScript owns the WebSocket and injects events into Rust via
`onConnected()`, `onBinaryMessage()`, etc. This is fundamentally inverted from the
`SyncTransport` async push/pull model (which requires `Send`, `recv()` blocking, etc.).

Both `WasmSyncClient` and `SyncClient` share the same `SyncSession` protocol handler.
The transport layer is where they diverge. This is correct by design. No changes to WASM.

---

## Verification Steps

1. `cargo check -p diaryx_core --features native-sync` — core compiles with new generics
2. `cargo check -p diaryx` — CLI compiles after removing dead dep and updating call site
3. `cargo check -p diaryx_tauri` — Tauri compiles after removing dead deps and updating call site
4. `cargo test -p diaryx_core` — existing tests pass
5. `cargo test -p diaryx` — CLI tests pass
6. Confirm only ONE copy of `tokio-tungstenite` in `cargo tree` output for any leaf crate

---

## Summary of Changes

| File | Change |
|------|--------|
| `crates/diaryx/Cargo.toml` | Remove `tokio-tungstenite = "0.24"` |
| `apps/tauri/src-tauri/Cargo.toml` | Remove `tokio-tungstenite = "0.23"` (and possibly `futures-util`, `url`) |
| `crates/diaryx_core/Cargo.toml` | Add `features = ["rustls-tls-webpki-roots"]` to tokio-tungstenite |
| `crates/diaryx_core/src/crdt/transport.rs` | Add `TransportConnector` trait |
| `crates/diaryx_core/src/crdt/tokio_transport.rs` | Add `TokioConnector` struct + impl |
| `crates/diaryx_core/src/crdt/sync_client.rs` | Make generic over `C: TransportConnector` |
| `crates/diaryx_core/src/crdt/mod.rs` | Re-export new types |
| `crates/diaryx/src/cli/sync/client.rs` | Pass `TokioConnector` to `SyncClient::new()` |
| `apps/tauri/src-tauri/src/commands.rs` | Pass `TokioConnector` to `SyncClient::new()` |
| `crates/diaryx_core/src/crdt/README.md` | Document TransportConnector |
| `apps/tauri/README.md` | Note transport swappability |
