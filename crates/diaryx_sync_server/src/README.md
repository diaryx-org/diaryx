---
title: diaryx_sync_server src
description: Source code for the sync server
part_of: '[README](/crates/diaryx_sync_server/README.md)'
contents:
- '[README](/crates/diaryx_sync_server/src/auth/README.md)'
- '[README](/crates/diaryx_sync_server/src/db/README.md)'
- '[README](/crates/diaryx_sync_server/src/email/README.md)'
- '[README](/crates/diaryx_sync_server/src/handlers/README.md)'
- '[README](/crates/diaryx_sync_server/src/sync_v2/README.md)'
exclude:
- '*.lock'
---

# diaryx_sync_server Source

This directory contains the source code for the Diaryx sync server.

`diaryx_sync_server` now acts as the native adapter around the shared
`diaryx_server` core so future platforms can bind to the same business logic
without pulling Axum or cloud-runtime details into the core.

Shared current-user and domain-management use cases now live in
`diaryx_server`, while this crate supplies SQLite-backed `NamespaceStore` /
`AuthStore` implementations plus a best-effort Cloudflare KV
`DomainMappingCache` adapter.

## Structure

| File | Purpose |
|------|---------|
| `adapters.rs` | Native implementations of the shared `diaryx_server` ports, including the Cloudflare KV cache adapter |
| `lib.rs` | Library entry point |
| `main.rs` | Server entry point |
| `config.rs` | Configuration from environment variables |
| `blob_store.rs` | Attachment blob storage adapter implementing the shared `diaryx_server::BlobStore` port (R2/in-memory) |
| `publish.rs` | Static site publishing pipeline + token signing helpers (audience-filtered builds using workspace config's `public_audience`, per-audience artifact replacement, root-index selection via `diaryx_core::workspace`, helper-tested audience discovery/normalization, and zero-build diagnostics) |

## Modules

- `auth/` - Authentication middleware and magic link handling
- `blob_store.rs` - R2-backed attachment blob storage adapter for the shared server core
- `db/` - SQLite database schema and repository
- `email/` - SMTP email sending
- `git_ops.rs` - Git operations (commit, restore) for server-side workspaces
- `handlers/` - HTTP route handlers
- `publish.rs` - CRDT materialization -> static HTML upload pipeline for published sites
- `sync_v2/` - Siphonophore-based sync implementation

## Operational Notes

- Git auto-commit now treats `No files to commit` as a non-retriable skip and clears the workspace dirty flag to avoid repeated 60s error spam.
- Auto-commit also ignores workspace IDs that no longer exist in `user_workspaces`, clearing dirty state and opportunistically removing orphan workspace `.db`/`.git` artifacts.
- Attachment reconciliation now derives candidate paths from both markdown links
  and raw HTML `src`/`href` attributes (with percent-decoding), which improves
  publish-time attachment URL rewriting for HTML-embedded media.
- Published site creation enforces `get_effective_published_site_limit(...)`.
  Free-tier defaults now include one published site, while per-user overrides
  can still raise or lower that limit for tests, seeds, or grandfathered
  accounts.
