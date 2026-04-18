//! End-to-end contract tests for `diaryx_cloudflare`.
//!
//! The [`testing`] module exposes the `WranglerDev` process fixture and a
//! dev-mode magic-link sign-in helper. They're in the library (not a test
//! file) so test binaries in `tests/` — `contract.rs`, `sync_plugin_e2e.rs`
//! — can share them instead of duplicating the spawn/teardown logic.

pub mod testing;
