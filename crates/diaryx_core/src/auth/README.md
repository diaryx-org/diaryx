# Auth module

Platform-agnostic authentication for the Diaryx sync server.

## Architecture

`AuthService<C>` is the single source of truth for the magic-link flow,
session management, device management, and workspace CRUD. All platform
differences are encapsulated in a concrete `AuthenticatedClient`
implementation that the service wraps:

```
AuthService<C: AuthenticatedClient>
└── AuthenticatedClient  — per-platform HTTP + credential storage
    ├── FsAuthenticatedClient       (CLI — auth.md frontmatter + ureq)
    ├── KeyringAuthenticatedClient  (Tauri — OS keyring + reqwest)
    └── WasmAuthenticatedClient     (Browser — JS callbacks + HttpOnly cookie)
```

Each impl owns its own HTTP transport and persists its own session token
somewhere platform-appropriate. The service never touches the raw token.

## Key types

| Type                   | Description                                                                     |
| ---------------------- | ------------------------------------------------------------------------------- |
| `AuthService`          | Magic link, `get_me`, device + workspace CRUD, account deletion                 |
| `AuthenticatedClient`  | Trait bundling authenticated HTTP + `store/clear_session_token` + metadata I/O |
| `AuthMetadata`         | Non-secret session metadata (`email`, `workspace_id`)                           |
| `AuthError`            | `{ message, status_code, devices }` — `devices` populated on 403 device-limit  |
| `MeResponse`           | Server user info: tier, workspace limit, devices, storage limit                 |
| `VerifyResponse`       | `{ success, token, user }` returned by magic-link / code verification           |

## Verify flow extras

`verify_magic_link` and `verify_code` both accept an optional
`replace_device_id` so frontends can surface the device picker when the
account is at its device limit. On a 403 response, the service parses the
server's `devices` array into `AuthError.devices`, letting UIs drive a
retry loop without reimplementing the parse.

## Concrete implementations

- **CLI** — `FsAuthenticatedClient` in `crates/diaryx/src/cli/auth_client.rs`
  persists the token into `~/.config/diaryx/auth.md` frontmatter (alongside
  `config.md`) and uses `ureq` for transport.
- **Tauri** — `KeyringAuthenticatedClient` in
  `apps/tauri/src-tauri/src/auth_client.rs` stores the token in the OS
  keyring (`org.diaryx.app`/`session_token`) and writes non-secret metadata
  (server URL + `email`/`workspace_id`) to `<app_data>/auth.json`. The
  twelve `AuthService` methods are exposed to the web layer via the
  `auth_*` Tauri IPC commands in `auth_commands.rs`.
- **Browser** — `WasmAuthenticatedClient` in `crates/diaryx_wasm/src/auth.rs`
  delegates HTTP to a JavaScript `fetch` callback (which runs
  `proxyFetch` with `credentials: 'include'` so the server's HttpOnly
  session cookie is always attached). The token itself never touches JS.
  The wasm-bindgen `AuthClient` class wraps `AuthService<WasmAuthenticatedClient>`.
