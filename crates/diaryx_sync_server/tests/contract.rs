//! Runs the shared [`diaryx_server::contract`] test suite against
//! `diaryx_sync_server`, using the same `ReqwestDispatcher` that the
//! cloudflare adapter uses against `wrangler dev`. The dispatcher hits a
//! real `TestServer` bound on `127.0.0.1:0` so every request exercises the
//! full router + Axum path extractors + SQLite — not a thin in-process
//! subset.
//!
//! Keeping this identical to the cloudflare wrapper means adapter drift
//! surfaces immediately:
//!
//!     cargo test -p diaryx_sync_server --test contract        # this
//!     cargo test -p diaryx_cloudflare_e2e --test contract -- --ignored
//!
//! Any scenario that passes one but not the other is drift.

use diaryx_server::contract::{self, http::ReqwestDispatcher};
use diaryx_sync_server::testing::TestServer;

/// Every test spins up its own fresh server: `:memory:` SQLite, new
/// in-memory blob store, clean slate. That's cheap (sub-second) and
/// eliminates cross-test pollution — the tradeoff the cloudflare side
/// can't make (one wrangler-dev per suite, all tests share state).
async fn fresh_dispatcher() -> (TestServer, ReqwestDispatcher) {
    let server = TestServer::start().await;
    let dispatcher = ReqwestDispatcher::new(server.base_url());
    (server, dispatcher)
}

// -- Health + auth (unchanged test coverage) ---------------------------------

#[tokio::test]
async fn contract_health_endpoint() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_health_endpoint_returns_200_ok(&d).await;
}

#[tokio::test]
async fn contract_magic_link_dev_mode() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_magic_link_dev_mode_returns_dev_credentials(&d).await;
}

#[tokio::test]
async fn contract_magic_link_rejects_invalid_email() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_magic_link_rejects_invalid_email(&d).await;
}

// -- Auth round-trip ---------------------------------------------------------

#[tokio::test]
async fn contract_magic_link_verify_returns_session() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_magic_link_verify_returns_session(&d).await;
}

#[tokio::test]
async fn contract_me_without_auth_returns_401() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_me_without_auth_returns_401(&d).await;
}

// -- Namespace lifecycle -----------------------------------------------------

#[tokio::test]
async fn contract_namespace_create_list_get_lifecycle() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_namespace_create_list_get_lifecycle(&d).await;
}

#[tokio::test]
async fn contract_namespace_access_forbidden_for_other_users() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_namespace_access_forbidden_for_other_users(&d).await;
}

// -- Object CRUD + URL fuzz --------------------------------------------------

#[tokio::test]
async fn contract_object_put_get_delete_roundtrip() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_object_put_get_delete_roundtrip(&d).await;
}

#[tokio::test]
async fn contract_object_list_returns_uploaded_keys() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_object_list_returns_uploaded_keys(&d).await;
}

#[tokio::test]
async fn contract_url_corpus_keys_roundtrip_through_http() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_url_corpus_keys_roundtrip_through_http(&d).await;
}

// -- Batch -------------------------------------------------------------------

#[tokio::test]
async fn contract_batch_objects_json_returns_all_keys() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_batch_objects_json_returns_all_keys(&d).await;
}

#[tokio::test]
async fn contract_batch_objects_multipart_returns_all_keys() {
    let (_s, d) = fresh_dispatcher().await;
    contract::test_batch_objects_multipart_returns_all_keys(&d).await;
}
