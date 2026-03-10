# Auth module

Platform-agnostic authentication for the Diaryx sync server.

## Architecture

`AuthService<H, S>` handles magic link authentication, session management,
and user info queries. Platform-specific HTTP and storage are injected via
the `AuthHttpClient` and `AuthStorage` traits.

```
AuthService
├── AuthHttpClient  — CLI: reqwest blocking, WASM: js-sys fetch
└── AuthStorage     — CLI: native auth file + legacy config fallback, WASM: localStorage
```

## Key types

| Type | Description |
|------|------------|
| `AuthService` | Main service with `request_magic_link()`, `verify_magic_link()`, `logout()`, `get_me()` |
| `AuthCredentials` | Stored credentials (server_url, session_token, email, workspace_id) |
| `AuthHttpClient` | Trait for platform-specific HTTP |
| `AuthStorage` | Trait for platform-specific credential persistence |
| `MeResponse` | Server user info including tier, workspace_limit, devices |

## CLI implementation

See `crates/diaryx/src/cli/sync/auth.rs` for `ReqwestAuthClient` and the CLI
integration with `NativeFileAuthStorage`. Native auth now persists to
`~/.config/diaryx/auth.toml` (or the platform equivalent) and only falls back
to legacy `Config.sync_*` fields for migration/compatibility.
