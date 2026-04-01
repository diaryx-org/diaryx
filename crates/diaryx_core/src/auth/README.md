# Auth module

Platform-agnostic authentication for the Diaryx sync server.

## Architecture

`AuthService<H, S>` handles magic link authentication, session management,
and user info queries. Platform-specific HTTP and storage are injected via
the `AuthHttpClient` and `AuthStorage` traits.

```
AuthService
├── AuthHttpClient  — host-provided native HTTP client or WASM fetch
└── AuthStorage     — native auth file + legacy config fallback, or browser localStorage
```

## Key types

| Type | Description |
|------|------------|
| `AuthService` | Main service with `request_magic_link()`, `verify_magic_link()`, `logout()`, `get_me()` |
| `AuthCredentials` | Stored credentials (server_url, session_token, email, workspace_id) |
| `AuthHttpClient` | Trait for platform-specific HTTP |
| `AuthStorage` | Trait for platform-specific credential persistence |
| `MeResponse` | Server user info including tier, workspace_limit, devices |

## Native hosts

`NativeFileAuthStorage` persists credentials to `~/.config/diaryx/auth.md`
(or the platform equivalent) as a markdown file with YAML frontmatter. The
file includes `part_of: config.md` so that the config directory forms a mini
Diaryx workspace. Falls back to legacy `auth.toml` and `Config.sync_*` fields
for migration/compatibility. CLI plugin hosts load these credentials into the
runtime context passed to sync-capable plugins.
