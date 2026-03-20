---
title: Handlers module
description: HTTP route handlers
part_of: "[README](/crates/diaryx_sync_server/src/README.md)"
attachments:
  - "[mod.rs](/crates/diaryx_sync_server/src/handlers/mod.rs)"
  - "[ai.rs](/crates/diaryx_sync_server/src/handlers/ai.rs)"
  - "[auth.rs](/crates/diaryx_sync_server/src/handlers/auth.rs)"
  - "[audiences.rs](/crates/diaryx_sync_server/src/handlers/audiences.rs)"
  - "[domains.rs](/crates/diaryx_sync_server/src/handlers/domains.rs)"
  - "[namespaces.rs](/crates/diaryx_sync_server/src/handlers/namespaces.rs)"
  - "[ns_sessions.rs](/crates/diaryx_sync_server/src/handlers/ns_sessions.rs)"
  - "[objects.rs](/crates/diaryx_sync_server/src/handlers/objects.rs)"
  - "[stripe.rs](/crates/diaryx_sync_server/src/handlers/stripe.rs)"
  - "[apple.rs](/crates/diaryx_sync_server/src/handlers/apple.rs)"
exclude:
  - "*.lock"
---

# Handlers Module

HTTP route handlers for the sync server API.

## Files

| File              | Purpose                                                       |
| ----------------- | ------------------------------------------------------------- |
| `mod.rs`          | Router setup, middleware, `require_namespace_owner` helper     |
| `ai.rs`           | Managed AI proxy endpoint (`/api/ai/chat/completions`)        |
| `auth.rs`         | Authentication endpoints (magic-link, verify, logout, cookies) |
| `audiences.rs`    | Audience visibility management endpoints                      |
| `domains.rs`      | Custom domain management + Caddy `forward_auth` endpoint, with registration flows delegated to shared `diaryx_server` use cases |
| `namespaces.rs`   | Namespace CRUD endpoints                                      |
| `ns_sessions.rs`  | Namespace session management endpoints                        |
| `objects.rs`      | Object store + public object access + usage endpoints         |
| `stripe.rs`       | Stripe billing endpoints (checkout, portal, webhook)          |
| `apple.rs`        | Apple IAP receipt verification endpoints                      |

### Workspace CRUD Endpoints

- `GET /api/workspaces` — list user's workspaces
- `POST /api/workspaces` — create workspace (body: `{"name": "..."}`, enforces per-user workspace limit)
- `GET /api/workspaces/{id}` — get workspace info
- `PATCH /api/workspaces/{id}` — rename workspace (body: `{"name": "..."}`)
- `DELETE /api/workspaces/{id}` — delete workspace + cleanup (attachment refs, dirty auto-commit state, cached CRDT handle eviction, workspace `.db` removal, git repo removal)

Workspace creation returns `403` when the user's workspace limit is reached, and `409` for duplicate names.
The per-user workspace limit defaults to the user's tier (Free=1, Plus=10) and can be overridden via `workspace_limit` on the users table.
The `GET /auth/me` response includes `workspace_limit`, `tier`, `published_site_limit`, and `attachment_limit_bytes`.
Auth verification endpoints (`/auth/verify`, `/auth/verify-code`, `/auth/passkeys/authenticate/finish`) set an `HttpOnly; Secure; SameSite=Strict` session cookie (`diaryx_session`) alongside the JSON token response. The middleware extracts auth from: (1) `Authorization: Bearer` header, (2) `Cookie: diaryx_session=<token>`, (3) `?token=` query param. The logout endpoint clears the cookie with `Max-Age=0`.

Auth verification also enforces the tier device cap during session creation; Free users can keep up to 2 registered devices and Plus users up to 10. When the limit is reached the `403` response includes a `devices` array with each device's `id`, `name`, and `last_seen_at`. The client can then re-submit the verification request with `replace_device_id` set to the ID of the device to remove, completing sign-in in one step.

### Snapshot Endpoints

`api.rs` also serves workspace snapshot downloads and uploads at
`GET /api/workspaces/{workspace_id}/snapshot` and
`POST /api/workspaces/{workspace_id}/snapshot`.
Snapshot endpoints support `include_attachments=true|false` query params.
Snapshot uploads also support `mode=replace|merge` and enforce a configurable
max payload size (`SNAPSHOT_UPLOAD_MAX_BYTES`, default 1 GiB).
Snapshot import ignores hidden/system ZIP metadata entries (for example
`__MACOSX/**`, `.DS_Store`, and `._*` sidecar files) so macOS-generated archives
do not fail markdown parsing.
Attachment-enabled imports process binaries in a two-pass flow (hash/size scan,
then one-by-one blob uploads) to avoid buffering all attachment payloads in
memory at once.

`api.rs` also serves per-user attachment usage at:

- `GET /api/user/storage` — returns used bytes/blob count for synced attachment blobs.
- `GET /api/user/has-data` — reports whether the user has synced files across
  any workspace (aggregated file count).
- `POST /api/workspaces/{workspace_id}/attachments/uploads` — initialize/resume multipart attachment upload.
- `PUT /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/parts/{part_no}` — upload one part.
- `POST /api/workspaces/{workspace_id}/attachments/uploads/{upload_id}/complete` — finalize multipart upload.
- Completing an upload now triggers immediate workspace attachment-ref
  reconciliation, so newly completed blobs become downloadable without waiting
  for another workspace metadata edit.
- Init/complete upload handlers canonicalize `attachment_path` against
  `entry_path` before storing/comparing upload sessions, so relative and
  canonical client path formats resolve to the same workspace attachment path.
- `already_exists` init responses now also persist a completed
  attachment-path→hash lookup session so reconciliation can still backfill refs
  for frontmatter/body paths that do not yet carry explicit hash metadata.
- `already_exists` now requires both blob metadata and a verifiable blob object;
  placeholder metadata rows (for example rows with empty `r2_key` created during
  ref reconciliation) no longer short-circuit uploads.

For one-part uploads, the handler uses a direct blob `put` path internally and
skips remote multipart completion/abort calls.
- `GET /api/workspaces/{workspace_id}/attachments/{hash}` — download attachment bytes (supports `Range`). Accepts `?token=` (owner auth) or `?session=CODE` (guest access via share session). Guest access validates session code, workspace match, and owner Plus subscription.

Attachment uploads and attachment-enabled snapshot imports enforce per-user
attachment limits. Over-limit requests return `413` with
`error: "storage_limit_exceeded"` JSON.

### Git Version History Endpoints

- `GET /api/workspaces/{id}/history?count=N` — Commit log from bare repo
- `POST /api/workspaces/{id}/commit` — Trigger immediate git commit (body: `{"message": "..."}`)
- `POST /api/workspaces/{id}/restore` — Rebuild CRDT from target commit (body: `{"commit_id": "..."}`, requires peer_count <= 1)

### Published Site Endpoints

- `POST /api/workspaces/{id}/site` — create published site config (`slug`, optional `enabled`, optional `auto_publish`).
- `GET /api/workspaces/{id}/site` — fetch site config + per-audience build status.
- `DELETE /api/workspaces/{id}/site` — unpublish site and delete static artifacts from the sites bucket.
- `POST /api/workspaces/{id}/site/publish` — deprecated legacy publish trigger. Prefer `POST /api/workspaces/{id}/site/publish-with-fallback`.
- `POST /api/workspaces/{id}/site/publish-with-fallback` — preferred publish endpoint. Publishes from server state when available and can fall back to an uploaded snapshot body.
- `POST /api/workspaces/{id}/site/tokens` — create signed access token (`audience`, optional `label`, optional `expires_in`).
- `GET /api/workspaces/{id}/site/tokens` — list token metadata for the workspace site.
- `DELETE /api/workspaces/{id}/site/tokens/{token_id}` — revoke a token and refresh `_meta.json` revocation list.

Published site creation uses `get_effective_published_site_limit(...)`. The
current tier defaults allow one published site on Free and one on Plus unless a
per-user override is set on `users.published_site_limit`.

### Stripe Billing Endpoints

Only available when `STRIPE_SECRET_KEY` is configured.

- `POST /api/stripe/checkout` — create Stripe Checkout Session for Plus upgrade. Returns `{ url }`. Requires auth.
- `POST /api/stripe/portal` — create Stripe Customer Portal session. Returns `{ url }`. Requires auth.
- `POST /api/stripe/webhook` — Stripe webhook handler (no auth, uses signature verification). Handles `checkout.session.completed`, `customer.subscription.updated`, `customer.subscription.deleted`.
- `GET /api/stripe/config` — returns `{ publishable_key }`. Public endpoint.

### Admin Endpoints

- `PUT /api/admin/users/{user_id}/tier` — set a user's tier (body: `{"tier": "free"|"plus"}`). Requires `X-Admin-Secret` header matching the `ADMIN_SECRET` env var. Returns `204` on success, `401` on bad secret, `404` if no admin secret configured or user not found, `400` on invalid tier.

### Managed AI Endpoint

- `POST /api/ai/chat/completions` — Authenticated managed AI proxy endpoint (OpenRouter upstream).
- Requires Plus tier (`plus_required`), validates model against allowlist (`model_not_allowed`), enforces per-user rate limit (`rate_limited`) and monthly quota (`quota_exceeded`), returns `provider_unavailable` for upstream/config failures.

### Namespace Object Endpoints

- `PUT /namespaces/{ns_id}/objects/{*key}` — store bytes (owner, `X-Audience` header sets audience tag)
- `GET /namespaces/{ns_id}/objects/{*key}` — retrieve bytes (owner)
- `DELETE /namespaces/{ns_id}/objects/{*key}` — delete object (owner)
- `GET /namespaces/{ns_id}/objects` — list object metadata (owner)

### Public Object Access

- `GET /public/{ns_id}/objects/{*key}` — unauthenticated object access, checks audience access level. For `token` audiences, pass `?audience_token=<token>`.

### Audience Endpoints

- `PUT /namespaces/{ns_id}/audiences/{name}` — set audience access level (owner)
- `GET /namespaces/{ns_id}/audiences` — list audiences (owner)
- `GET /namespaces/{ns_id}/audiences/{name}/token` — generate signed access token (owner)
- `DELETE /namespaces/{ns_id}/audiences/{name}` — remove audience, NULLs out audience on referencing objects (owner)

### Custom Domain Endpoints

- `PUT /namespaces/{ns_id}/domains/{domain}` — register custom domain (owner, body: `{ audience_name }`)
- `GET /namespaces/{ns_id}/domains` — list custom domains (owner)
- `DELETE /namespaces/{ns_id}/domains/{domain}` — remove custom domain (owner)
- `PUT /namespaces/{ns_id}/subdomain` — claim `https://{subdomain}.diaryx.org` and sync the edge cache (owner, body: `{ subdomain, default_audience? }`)
- `DELETE /namespaces/{ns_id}/subdomain` — release the namespace's Diaryx subdomain and remove the edge cache entry (owner)
- `GET /domain-auth` — Caddy `forward_auth` endpoint (unauthenticated, reads `X-Forwarded-Host` + `X-Forwarded-Uri`)

The domain mutation routes now call shared Rust core flows that validate
audiences/labels, update `custom_domains`, and invoke the `DomainMappingCache`
port so native and future cloud adapters can share the same behavior.
