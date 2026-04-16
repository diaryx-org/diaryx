---
title: diaryx_cloudflare_e2e
description: End-to-end contract tests that drive `diaryx_cloudflare` through `wrangler dev`
author: adammharris
part_of: '[README](/crates/README.md)'
exclude:
- '*.lock'
---

# diaryx_cloudflare_e2e

A test-only crate whose sole purpose is to drive
[`diaryx_cloudflare`](../diaryx_cloudflare) through `wrangler dev --local`
and assert against the shared
[`diaryx_server::contract`](../diaryx_server/src/contract/mod.rs) suite.

## Why a separate crate?

`diaryx_cloudflare` builds only for `wasm32-unknown-unknown` (its
`#[async_trait(?Send)]` impls are incompatible with native-target trait
`Send` bounds from `diaryx_server::ports`). That means native-target test
code can't live inside that crate. This sibling crate holds the wrangler-dev
harness and the native-target test binary.

## Running

The single test is `#[ignore]`-gated because it spawns a real `wrangler dev`
process (~30s+ first-time startup while `worker-build` runs).

Prerequisites:

- [Bun](https://bun.sh) installed and on `PATH` (we use `bunx wrangler` so
  no separate wrangler install is needed).

Run:

```bash
cargo test -p diaryx_cloudflare_e2e --test contract -- --ignored --nocapture
```

Environment variables:

- `DIARYX_CF_TEST_PORT` — port to bind wrangler to (default `8789`).
- `DIARYX_CF_RESET_D1=1` — wipe the local D1 state
  (`crates/diaryx_cloudflare/.wrangler/state/v3/d1/`) before applying
  migrations. Use when a prior partial run left the DB inconsistent
  (migrations apply failing with `duplicate column name` / `already
  exists`). Destroys local dev data — only set when you know that's fine.

The test spawns `bunx wrangler dev --local --port <port>` in the
`diaryx_cloudflare` crate directory, polls `http://127.0.0.1:<port>/api/health`
until it returns 2xx (timeout 180s), then runs the shared contract suite
against it. On teardown the child process is killed.

## What's covered

Whatever's in [`diaryx_server::contract`] at the time of the run. Current
seeds:

- `test_health_endpoint_returns_200_ok`
- `test_magic_link_dev_mode_returns_dev_credentials`
- `test_magic_link_rejects_invalid_email`

Adding a new contract test upstream automatically extends both this test
binary and the one in [`diaryx_sync_server/tests/contract.rs`] — that's the
seam that makes adapter drift surface as test failures instead of
production incidents.
