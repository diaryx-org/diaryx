---
title: Y-sync v2 Module
description: Siphonophore-based sync implementation (experimental)
part_of: '[README](/crates/diaryx_sync_server/src/README.md)'
attachments:
  - '[mod.rs](/crates/diaryx_sync_server/src/sync_v2/mod.rs)'
  - '[hooks.rs](/crates/diaryx_sync_server/src/sync_v2/hooks.rs)'
  - '[server.rs](/crates/diaryx_sync_server/src/sync_v2/server.rs)'
  - '[handshake.rs](/crates/diaryx_sync_server/src/sync_v2/handshake.rs)'
---

# Y-sync v2 Module

Experimental sync implementation using the [siphonophore](https://github.com/gluonDB/siphonophore) library.

## Status

**Experimental** - Available at `/sync2` endpoint for testing. Production traffic should use `/sync` (v1).

## Features

| Feature | v1 (`/sync`) | v2 (`/sync2`) |
|---------|--------------|---------------|
| Workspace metadata sync | ✅ | ✅ |
| Body document sync | ✅ | ✅ |
| Native multiplexing | Custom | Native |
| Files-Ready handshake | ✅ | ✅ (hook-based) |
| `sync_complete` control signal | ✅ | ✅ |
| Focus tracking | ✅ | ❌ |
| Peer events | ✅ | ✅ (hook-based) |
| Session/guest support | ✅ | Partial |

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     sync_v2 module                            │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────────────┐ │
│  │ DiaryxHook  │   │ SyncV2Server│   │ Handshake (future)  │ │
│  │             │   │             │   │                     │ │
│  │ - auth      │──▶│ - axum      │   │ - FilesReady        │ │
│  │ - load doc  │   │   router    │   │ - Control messages  │ │
│  │ - on_change │   │ - siphon-   │   │ - Focus tracking    │ │
│  │ - on_save   │   │   ophore    │   │                     │ │
│  └─────────────┘   └─────────────┘   └─────────────────────┘ │
│                                                               │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────┐
│                    siphonophore library                       │
├──────────────────────────────────────────────────────────────┤
│  - Y-sync protocol handling                                   │
│  - Document multiplexing (native)                             │
│  - Actor-based architecture (Kameo)                           │
│  - Awareness support                                          │
└──────────────────────────────────────────────────────────────┘
```

## Document Namespacing

Documents are namespaced to distinguish workspace metadata from file body content:

```
workspace:<workspace_id>      # Workspace metadata CRDT
body:<workspace_id>/<path>    # File body content CRDT
```

Example:
```
workspace:abc123              # Metadata for workspace abc123
body:abc123/journal/2024.md   # Body of journal/2024.md file
```

## Wire Protocol

Siphonophore uses a simple multiplexing format:

```
[doc_id_len: u8][doc_id: bytes][yjs_payload: bytes]
```

This differs slightly from v1 which uses varuint encoding for the length.

## Usage

The `/sync2` endpoint is automatically mounted when the server starts:

```rust
// In main.rs
let sync_v2_server = SyncV2Server::new(repo.clone(), workspaces_dir.clone());
let sync_v2_router = sync_v2_server.into_router_at("/sync2");

let app = Router::new()
    // ... other routes
    .merge(sync_v2_router)
```

### Client Connection

```javascript
// Connect to sync2 endpoint with auth token
const ws = new WebSocket(`wss://server/sync2?token=${token}`);

// Send document access with multiplexed format
function sendToDoc(docId, payload) {
  const docIdBytes = new TextEncoder().encode(docId);
  const message = new Uint8Array(1 + docIdBytes.length + payload.length);
  message[0] = docIdBytes.length;
  message.set(docIdBytes, 1);
  message.set(payload, 1 + docIdBytes.length);
  ws.send(message);
}

// Access workspace metadata
sendToDoc('workspace:abc123', syncStep1Message);

// Access file body
sendToDoc('body:abc123/journal/2024.md', syncStep1Message);
```

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Module entry point and exports |
| `hooks.rs` | DiaryxHook implementation for siphonophore |
| `server.rs` | SyncV2Server wrapper |
| `handshake.rs` | Files-Ready handshake (future use) |

## Limitations

1. **Handshake is hook-emulated** - Files-Ready and `sync_complete` are implemented via hook control messages, not a strict transport-level pre-sync gate.
2. **Focus tracking broadcast** - Focus/unfocus messages are accepted but not fully relayed to peers.
3. **Session context** - Guest read-only enforcement happens at the hook level and remains less mature than v1.

## Future Work

- Improve focus list relay parity with v1
- Tighten guest/session behavior parity with v1
- Continue evaluating transport-level handshake controls
