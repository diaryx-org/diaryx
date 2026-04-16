---
title: diaryx_server
description: Platform-agnostic server core for Diaryx cloud adapters
author: adammharris
part_of: '[README](/crates/README.md)'
contents:
- '[README](/crates/diaryx_server/src/README.md)'
exclude:
- '*.lock'
---
# diaryx_server

Platform-agnostic Rust core for Diaryx server-side business logic.

This crate is intentionally independent from HTTP frameworks and cloud/runtime
bindings. It exposes shared domain types, capability traits, and use cases that
can be reused by native adapters (`diaryx_sync_server`) and future Cloudflare
adapters.

The shared core now includes typed `ServerCoreError` variants plus portable
domain-management flows so adapters can map consistent outcomes without relying
on string-matched error handling.

## Testing

Two public modules exist for cross-adapter testing:

- [`testing`](src/testing/mod.rs) — thread-safe, in-memory implementations of
  the [`ports`](src/ports.rs) traits (`InMemoryNamespaceStore`,
  `InMemoryObjectMetaStore`, `InMemoryBlobStore`). Use these to drive the
  portable use-case services (e.g. `ObjectService`) without a real database.
  See [tests/object_service.rs](tests/object_service.rs) for examples.

- [`contract`](src/contract/mod.rs) — shared HTTP-level contract tests that
  **any** server adapter must pass. Includes the `URL_KEY_CORPUS` fixture of
  URL-encoding edge cases. Adapters wire up an `HttpDispatcher` and invoke
  the shared tests against their own router. Current consumers:
  - [`diaryx_sync_server/tests/contract.rs`](../diaryx_sync_server/tests/contract.rs)
    — in-process Axum via `tower::ServiceExt::oneshot`.
  - [`diaryx_cloudflare_e2e`](../diaryx_cloudflare_e2e) — `wrangler dev --local`
    via [`contract::http::ReqwestDispatcher`] (gated behind the
    `reqwest-dispatcher` Cargo feature).

The `testing` module compiles unconditionally. The `contract::http` submodule
requires the `reqwest-dispatcher` feature.

Current status is **seed**: one in-process `ObjectService` test suite and
three contract tests (health + two magic-link flows). Extend the modules in
place; downstream adapters pick up new checks automatically.
