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

Both test binaries are `#[ignore]`-gated because each spawns a real
`wrangler dev` process (~60s first-time startup while `worker-build`
compiles the WASM worker).

Prerequisites:

- [Bun](https://bun.sh) installed and on `PATH` (we use `bunx wrangler` so
  no separate wrangler install is needed).
- Sync plugin WASM built (required for `sync_plugin_e2e.rs`):
  `cargo build -p diaryx_sync_extism --target wasm32-unknown-unknown --release`.

Run:

```bash
# HTTP contract suite
cargo test -p diaryx_cloudflare_e2e --test contract -- --ignored --nocapture

# Sync plugin E2E suite
cargo test -p diaryx_cloudflare_e2e --test sync_plugin_e2e -- --ignored --nocapture
```

Environment variables:

- `DIARYX_CF_TEST_PORT` — port to bind wrangler to. Defaults: `8789` for
  `contract`, `8790` for `sync_plugin_e2e` (so both suites can run back
  to back on distinct workerd instances).
- `DIARYX_CF_RESET_D1=1` — wipe the local D1 state
  (`crates/diaryx_cloudflare/.wrangler/state/v3/d1/`) before applying
  migrations. Use when a prior partial run left the DB inconsistent
  (migrations apply failing with `duplicate column name` / `already
  exists`). Destroys local dev data — only set when you know that's fine.

Both binaries spawn `bunx wrangler dev --env dev --local --port <port>`
in the `diaryx_cloudflare` crate directory, poll
`http://127.0.0.1:<port>/api/health` until it returns 2xx (timeout 180s),
then run their scenarios against it. On teardown the child process is
killed. Note: the `Drop` impl kills only the wrangler CLI — orphaned
`workerd` children sometimes survive; `pkill -9 -f workerd` is a reliable
cleanup if the port is reported as already bound on a fresh run.

### Amortising the boot — multi-scenario suites

Both test binaries consolidate every scenario into a **single** test
function that boots wrangler once, runs each scenario under
`catch_unwind`, and panics at the end with a summary of every failure.
This means a single drift doesn't abort the rest (you see all breakage in
one ~90s run) and the 60s wrangler boot is paid once — not once per
`#[test]`.

## What's covered

### `tests/contract.rs` — shared HTTP contract suite

Runs every test from `diaryx_server::contract` against the worker. Covers:

- Health + auth flow (magic-link request, verify, `/me` guard).
- Namespace lifecycle (create / list / get, cross-user forbidden).
- Object CRUD + URL-encoding fuzz through real HTTP routing.
- Batch fetch (JSON + multipart).
- `list_objects` response shape parity (every entry includes
  `content_hash` — the sync plugin needs this to detect remote changes).

Adding a new contract test upstream automatically extends this binary
*and* the sync_server contract tests — that's the seam that makes adapter
drift surface as test failures. Historical drifts found by this layer:

- Missing `/batch/objects/multipart` endpoint on cloudflare — fixed; both
  adapters now emit a byte-identical multipart body.
- `list_objects` hand-rolled JSON dropping `content_hash` — fixed;
  contract suite now asserts every entry carries it so any future drop
  fails fast.

### `tests/sync_plugin_e2e.rs` — sync plugin against the worker

Loads the real sync plugin WASM (`diaryx_sync_extism`) and drives
`HttpNamespaceProvider` instances against `bunx wrangler dev` via the
same dev-mode magic-link sign-in as the sync_server E2E. Catches
adapter-specific bugs (URL routing in worker-rs, D1 persistence, R2 vs
SQLite blob semantics) that the sync_server E2E can't see.

Mirrors the scenarios in
[`crates/plugins/diaryx_sync_extism/tests/sync_e2e.rs`](../plugins/diaryx_sync_extism/tests/sync_e2e.rs)
one-for-one; see that crate's README for the full scenario catalogue
(currently 13 scenarios — happy paths, conflict resolution, idempotence,
authz, URL/binary/large-payload fuzz, reconnect, concurrent sync).

Uses port **8790** by default (contract uses 8789) so they can run
back-to-back; override with `DIARYX_CF_TEST_PORT`.

## Library surface

`WranglerDev` (spawns wrangler, polls `/api/health`, applies D1
migrations) and `sign_in_dev` (dev-mode magic-link → verify helper) live
in [`src/testing/mod.rs`](src/testing/mod.rs) so both test binaries
share them. The `sign_in_dev` helper also works against sync_server,
making it reusable for any cross-adapter scenario.
