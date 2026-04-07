# diaryx_plugin_sdk

SDK for building Diaryx Extism WASM plugins.

This crate provides the guest-side types, host function wrappers, and state
management helpers needed to write a Diaryx plugin. It replaces per-plugin
boilerplate: protocol types, host function bindings, and config handling.

Published to [crates.io](https://crates.io/crates/diaryx_plugin_sdk) under the
MIT license so third-party plugin authors can depend on it.

## Quick start

```toml
[dependencies]
diaryx_plugin_sdk = { version = "1.4", features = ["full"] }
extism-pdk = "1.4"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

See the module-level docs in `src/lib.rs` for a full example.

## Feature flags

| Feature   | What it enables                          |
|-----------|------------------------------------------|
| `core`    | File I/O, storage, logging, timestamps   |
| `http`    | HTTP requests via the host               |
| `secrets` | Plugin-scoped secret storage             |
| `ws`      | WebSocket bridge                         |
| `events`  | Event emission to the host               |
| `plugins` | Inter-plugin command execution           |
| `context` | Runtime context queries                  |
| `wasi`    | WASI module execution                    |
| `files`    | User-provided file requests              |
| `namespaces` | Namespace object operations and namespace listing |
| `full`     | All of the above                         |

## Building

```bash
cargo check -p diaryx_plugin_sdk
```

This crate compiles as `rlib` — it is a library dependency for guest plugins,
not a standalone WASM module.
