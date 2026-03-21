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
- '[use_cases/auth.rs](/crates/diaryx_server/src/use_cases/auth.rs)'
- '[schema/mod.rs](/crates/diaryx_server/src/schema/mod.rs)'
exclude:
- '*.lock'
---
# diaryx_server Source

The core is split into:

- `domain.rs` - shared server-side models and limits
- `ports.rs` - capability traits (`NamespaceStore`, `SessionStore`, `BlobStore`, `AuthStore`, etc.) plus typed `ServerCoreError` variants that adapters implement and map
- `schema/` - canonical database schema and migrations (SQLite dialect), consumed by all server adapters
- `use_cases/current_user.rs` - portable account/session aggregation for `/auth/me`
- `use_cases/domains.rs` - portable custom-domain and Diaryx subdomain registration/removal flows backed by `NamespaceStore` + `DomainMappingCache`
- `use_cases/namespaces.rs` - portable namespace CRUD with ownership verification
- `use_cases/audiences.rs` - portable audience CRUD with access validation and `_audiences.json` blob metadata writing
- `use_cases/sessions.rs` - portable namespace session CRUD with ownership verification
- `use_cases/objects.rs` - portable object store CRUD (put/get/delete/list) with ownership checks, audience validation, blob operations, usage recording, and public access resolution
- `use_cases/auth.rs` - `SessionValidationService` for token validation + device heartbeat, plus `extract_token` for framework-agnostic token extraction from headers/cookies/query

No module in this crate depends on Axum, Cloudflare Worker bindings, or SQLite at compile time. (`rusqlite` is a dev-dependency used only for schema validation tests.)

## Database Migrations

The `schema/` module is the single source of truth for the database schema. It contains numbered `.sql` migration files and exposes them as `MIGRATIONS` in `schema/mod.rs`. Both the native sync server (`diaryx_sync_server`) and the Cloudflare adapter (`diaryx_cloudflare`) consume these migrations.

### Adding a new migration

1. **Create the SQL file** in `src/schema/` following the naming convention `NNNN_short_name.sql` (e.g. `0004_add_foo.sql`). Write standard SQLite DDL — `ALTER TABLE`, `CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF NOT EXISTS`, etc.

2. **Register it** in `src/schema/mod.rs`: add an entry to the `MIGRATIONS` array and bump `CURRENT_VERSION`.

3. **Copy the file** to `crates/diaryx_cloudflare/migrations/` with the same filename. D1 uses its own migration runner, so it needs its own copy.

4. **Run tests** — `cargo test -p diaryx_server --lib schema` will verify:
   - Migrations are sequentially numbered.
   - All migrations apply cleanly to an empty database.
   - The Cloudflare migration files produce an identical schema to the canonical migrations (catches copy mistakes or drift).

5. **Sync server** — no code changes needed. `diaryx_sync_server` tracks `PRAGMA user_version` and automatically applies any migration whose version is greater than the stored version on next startup.

### How each adapter consumes migrations

| Adapter | Mechanism |
|---|---|
| `diaryx_sync_server` | `db::schema::init_database()` reads `PRAGMA user_version`, applies pending migrations from `diaryx_server::schema::MIGRATIONS`, updates `user_version`. Legacy (pre-versioned) databases are detected and brought up to date via `legacy_migrate()`. |
| `diaryx_cloudflare` | D1 migration files in `crates/diaryx_cloudflare/migrations/`. Applied by `wrangler d1 migrations apply`. Must be kept identical to the canonical SQL files. |

### Non-SQLite adapters

The migrations are SQLite-specific. A future adapter using a different SQL engine should implement the port traits in `ports.rs` (the authoritative contract) and write its own DDL. The canonical migrations serve as a reference for the expected tables, columns, and relationships.
