---
title: diaryx_server src
description: Platform-agnostic core modules for Diaryx server adapters
part_of: '[README](/crates/diaryx_server/README.md)'
attachments:
- '[lib.rs](/crates/diaryx_server/src/lib.rs)'
- '[domain.rs](/crates/diaryx_server/src/domain.rs)'
- '[ports.rs](/crates/diaryx_server/src/ports.rs)'
- '[use_cases/current_user.rs](/crates/diaryx_server/src/use_cases/current_user.rs)'
- '[use_cases/domains.rs](/crates/diaryx_server/src/use_cases/domains.rs)'
- '[use_cases/namespaces.rs](/crates/diaryx_server/src/use_cases/namespaces.rs)'
- '[use_cases/audiences.rs](/crates/diaryx_server/src/use_cases/audiences.rs)'
- '[use_cases/sessions.rs](/crates/diaryx_server/src/use_cases/sessions.rs)'
- '[use_cases/objects.rs](/crates/diaryx_server/src/use_cases/objects.rs)'
exclude:
- '*.lock'
---
# diaryx_server Source

The core is split into:

- `domain.rs` - shared server-side models and limits
- `ports.rs` - capability traits (`NamespaceStore`, `SessionStore`, `BlobStore`, `AuthStore`, etc.) plus typed `ServerCoreError` variants that adapters implement and map
- `use_cases/current_user.rs` - portable account/session aggregation for `/auth/me`
- `use_cases/domains.rs` - portable custom-domain and Diaryx subdomain registration/removal flows backed by `NamespaceStore` + `DomainMappingCache`
- `use_cases/namespaces.rs` - portable namespace CRUD with ownership verification
- `use_cases/audiences.rs` - portable audience CRUD with access validation and `_audiences.json` blob metadata writing
- `use_cases/sessions.rs` - portable namespace session CRUD with ownership verification
- `use_cases/objects.rs` - portable object store CRUD (put/get/delete/list) with ownership checks, audience validation, blob operations, usage recording, and public access resolution

No module in this crate depends on Axum, Cloudflare Worker bindings, or SQLite.
