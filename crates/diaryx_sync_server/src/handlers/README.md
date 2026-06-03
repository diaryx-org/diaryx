---
title: Handlers module
description: HTTP route handlers
part_of: "[README](/crates/diaryx_sync_server/src/README.md)"
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

### Auth Endpoints

Auth verification endpoints (`/auth/verify`, `/auth/verify-code`, `/auth/passkeys/authenticate/finish`) set an `HttpOnly; Secure; SameSite=Strict` session cookie (`diaryx_session`) alongside the JSON token response. The middleware extracts auth from: (1) `Authorization: Bearer` header, (2) `Cookie: diaryx_session=<token>`, (3) `?token=` query param. The logout endpoint clears the cookie with `Max-Age=0`.

Auth verification also enforces the tier device cap during session creation; Free users can keep up to 2 registered devices and Plus users up to 10. When the limit is reached the `403` response includes a `devices` array with each device's `id`, `name`, and `last_seen_at`. The client can then re-submit the verification request with `replace_device_id` set to the ID of the device to remove, completing sign-in in one step.

Device rename/delete endpoints return JSON `error` bodies for validation and
ownership failures. In particular, deleting the current session's device
returns `400` with an explanation to sign out on that device instead, so
settings UIs can show the actual cause.

### Namespace Endpoints

- `POST /api/namespaces` — create a namespace, optionally with a caller-supplied ID and metadata.
- `GET /api/namespaces` — list namespaces owned by the current user.
- `GET /api/namespaces/{id}` — fetch one owned namespace.
- `PATCH /api/namespaces/{id}` — replace namespace metadata.
- `DELETE /api/namespaces/{id}` — delete a namespace and its metadata.

Workspace-like containers are represented as namespaces, usually with metadata
such as `{ "type": "workspace" }`. The deprecated `/api/workspaces/*`,
snapshot, and multipart attachment routes are no longer mounted.

### Namespace Object Endpoints

- `GET /api/namespaces/{ns_id}/objects` — list object metadata.
- `PUT /api/namespaces/{ns_id}/objects/{*key}` — store bytes. Owner auth required; optional `X-Audience` tags the object.
- `GET /api/namespaces/{ns_id}/objects/{*key}` — fetch bytes. Owner auth required.
- `DELETE /api/namespaces/{ns_id}/objects/{*key}` — delete an object.
- `POST /api/namespaces/{ns_id}/batch/objects` — JSON batch object fetch.
- `POST /api/namespaces/{ns_id}/batch/objects/multipart` — multipart batch object fetch.
- `GET /api/public/{ns_id}/objects/{*key}` — unauthenticated object access with audience-gate checks.
- `GET /api/usage` — user-level usage totals.
- `GET /api/namespaces/{ns_id}/usage` — namespace-level usage totals.

### Audience, Domain, and Session Endpoints

- `PUT /api/namespaces/{ns_id}/audiences/{name}` — set audience access gates.
- `GET /api/namespaces/{ns_id}/audiences` — list audiences.
- `GET /api/namespaces/{ns_id}/audiences/{name}/token` — create a signed audience token.
- `POST /api/namespaces/{ns_id}/audiences/{name}/unlock` — unlock a password-gated audience.
- `POST /api/namespaces/{ns_id}/audiences/{name}/rotate-password` — rotate an audience password gate.
- `DELETE /api/namespaces/{ns_id}/audiences/{name}` — remove an audience and clear references on objects.
- `GET /api/namespaces/{ns_id}/domains` — list custom domains.
- `PUT /api/namespaces/{ns_id}/domains/{domain}` — register a domain for an audience.
- `DELETE /api/namespaces/{ns_id}/domains/{domain}` — remove a domain.
- `PUT /api/namespaces/{ns_id}/subdomain` — claim a Diaryx subdomain.
- `DELETE /api/namespaces/{ns_id}/subdomain` — release a Diaryx subdomain.
- `GET /domain-auth` — Caddy `forward_auth` endpoint.
- `GET /domain-check` — domain mapping probe endpoint.
- `POST /api/sessions`, `GET /api/sessions/{code}`, `DELETE /api/sessions/{code}` — namespace share sessions.

The domain mutation routes call shared Rust core flows that validate
audiences/labels, update `custom_domains`, and invoke the `DomainMappingCache`
port so native and future cloud adapters can share the same behavior.

### Stripe Billing Endpoints

Only available when `STRIPE_SECRET_KEY` is configured.

- `POST /api/stripe/checkout` — create Stripe Checkout Session for Plus upgrade. Returns `{ url }`. Requires auth.
- `POST /api/stripe/portal` — create Stripe Customer Portal session. Returns `{ url }`. Requires auth.
- `POST /api/stripe/webhook` — Stripe webhook handler (no auth, uses signature verification). Handles `checkout.session.completed`, `customer.subscription.updated`, `customer.subscription.deleted`.
- `GET /api/stripe/config` — returns `{ publishable_key }`. Public endpoint.

### Managed AI Endpoint

- `POST /api/ai/chat/completions` — Authenticated managed AI proxy endpoint (OpenRouter upstream).
- Requires Plus tier (`plus_required`), validates model against allowlist (`model_not_allowed`), enforces per-user rate limit (`rate_limited`) and monthly quota (`quota_exceeded`), returns `provider_unavailable` for upstream/config failures.
