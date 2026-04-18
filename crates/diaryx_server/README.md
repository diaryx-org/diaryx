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

Current contract suite covers 12 scenarios:

- **Health + auth**: `health_endpoint_returns_200_ok`,
  `magic_link_dev_mode_returns_dev_credentials`,
  `magic_link_rejects_invalid_email`,
  `magic_link_verify_returns_session`, `me_without_auth_returns_401`.
- **Namespace lifecycle**: `namespace_create_list_get_lifecycle`,
  `namespace_access_forbidden_for_other_users`.
- **Object CRUD + URL fuzz**: `object_put_get_delete_roundtrip`,
  `object_list_returns_uploaded_keys` (asserts every entry carries
  `content_hash` — the sync plugin's `compute_diff` needs it to detect
  remote changes; omitting it silently breaks cross-device pull),
  `url_corpus_keys_roundtrip_through_http`.
- **Batch**: `batch_objects_json_returns_all_keys`,
  `batch_objects_multipart_returns_all_keys`. Both endpoints now exist
  on both adapters.

All authenticated tests share a `sign_in_via_magic_link(dispatcher, email)`
helper that does the dev-mode magic-link → verify dance against whatever
base URL the dispatcher talks to. Add new contract tests to
[`src/contract/mod.rs`](src/contract/mod.rs); both adapter wrappers pick
them up automatically once invocations land in
`diaryx_sync_server/tests/contract.rs` and
`diaryx_cloudflare_e2e/tests/contract.rs`.
