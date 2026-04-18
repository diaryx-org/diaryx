//! Runs the shared [`diaryx_server::contract`] suite against
//! `diaryx_cloudflare` served by `wrangler dev --env dev --local`.
//!
//! Each contract test is wrapped in `catch_unwind` so a single drift
//! doesn't abort the rest of the suite — with wrangler taking ~60s to
//! boot, we want to see *every* divergence in one run. Failures are
//! collected into a summary and the overall test panics at the end if
//! any sub-test failed.

#![cfg(not(target_arch = "wasm32"))]

use std::panic::AssertUnwindSafe;

use diaryx_cloudflare_e2e::testing::WranglerDev;
use diaryx_server::contract::{self, http::ReqwestDispatcher};
use futures::FutureExt;

/// Run a single contract test, capturing any panic so the suite continues.
/// Emits `✓` / `✗` lines so the user can see progress under `--nocapture`.
async fn run_one<F, Fut>(name: &str, make_fut: F, failures: &mut Vec<String>)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    eprintln!("▶ {name}");
    let fut = AssertUnwindSafe(make_fut());
    match fut.catch_unwind().await {
        Ok(()) => eprintln!("  ✓ {name}"),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            // Keep the panic line concise; the full body is already in
            // the test log above the summary.
            let first_line = msg.lines().next().unwrap_or("").to_string();
            eprintln!("  ✗ {name}: {first_line}");
            failures.push(format!("{name}: {first_line}"));
        }
    }
}

/// All contract tests run against one wrangler instance. Individual
/// failures are collected; the final assertion summarizes them.
#[tokio::test]
#[ignore]
async fn cloudflare_contract_suite() {
    let Some(server) = WranglerDev::spawn().await else {
        return;
    };
    let dispatcher = ReqwestDispatcher::new(server.base_url());

    let mut failures: Vec<String> = Vec::new();

    // Helper macro to reduce boilerplate for each invocation. Each test is
    // generic over `D: HttpDispatcher`, so we pass `&dispatcher` directly
    // (no `dyn` object — monomorphization gives each call its own copy).
    macro_rules! run {
        ($name:ident) => {
            run_one(
                stringify!($name),
                || contract::$name(&dispatcher),
                &mut failures,
            )
            .await
        };
    }

    // -- Health + auth -----------------------------------------------------
    run!(test_health_endpoint_returns_200_ok);
    run!(test_magic_link_dev_mode_returns_dev_credentials);
    run!(test_magic_link_rejects_invalid_email);
    run!(test_magic_link_verify_returns_session);
    run!(test_me_without_auth_returns_401);

    // -- Namespace lifecycle ----------------------------------------------
    run!(test_namespace_create_list_get_lifecycle);
    run!(test_namespace_access_forbidden_for_other_users);

    // -- Object CRUD + URL fuzz -------------------------------------------
    run!(test_object_put_get_delete_roundtrip);
    run!(test_object_list_returns_uploaded_keys);
    run!(test_url_corpus_keys_roundtrip_through_http);

    // -- Batch ------------------------------------------------------------
    run!(test_batch_objects_json_returns_all_keys);
    run!(test_batch_objects_multipart_returns_all_keys);

    // -- Summary ----------------------------------------------------------
    if failures.is_empty() {
        eprintln!("✓ all contract tests passed");
    } else {
        eprintln!("\n=== Contract drift summary ({} / ?) ===", failures.len());
        for f in &failures {
            eprintln!("  ✗ {f}");
        }
        panic!(
            "{} contract tests diverged between adapters; see the panic messages \
             above each '✗' line for full assertion context.",
            failures.len()
        );
    }
}
