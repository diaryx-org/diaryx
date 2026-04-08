# diaryx_extism

Extism-based third-party plugin runtime for Diaryx.

Loads WebAssembly plugin modules via the [Extism](https://extism.org/) runtime
and adapts them to `diaryx_core` plugin traits. Guest plugins communicate with
the host through a JSON protocol (`protocol.rs`).

## Plugin directory structure

```
~/.diaryx/plugins/
  my-plugin/
    plugin.wasm      # The WASM module
    manifest.json    # Cached guest manifest
    config.json      # Plugin config sidecar
```

## Guest-exported functions

| Function | Input | Output | When called |
|----------|-------|--------|-------------|
| `manifest` | `""` | `GuestManifest` JSON | At load time |
| `init` | `PluginContext` JSON | `""` | Plugin initialization |
| `on_event` | `GuestEvent` JSON | `""` | File/workspace events |
| `handle_command` | `CommandRequest` JSON | `CommandResponse` JSON | Command dispatch |
| `get_config` | `""` | config JSON | Config read |
| `set_config` | config JSON | `""` | Config write |

`GuestManifest` supports optional fields:

- `requested_permissions` — install-time permission defaults and rationale text
- `min_app_version` — minimum Diaryx version required (e.g. `"1.4.0"`);
  the loader rejects plugins when the running app is too old

## Host functions available to guests

| Function | Description |
|----------|-------------|
| `host_log` | Plugin logging |
| `host_read_file` / `host_list_files` / `host_file_exists` | Workspace reads |
| `host_write_file` / `host_write_binary` | Workspace writes (create/edit split by file existence) |
| `host_delete_file` | Workspace deletes |
| `host_request_file` | Host-provided selected file bytes (raw bytes; no base64 wrapper on current hosts) |
| `host_http_request` | HTTP request bridge (feature-gated, supports optional `timeout_ms`) |
| `host_storage_get` / `host_storage_set` | Plugin persistent storage |
| `host_run_wasi_module` | Execute a WASI module loaded from plugin storage (feature-gated) |
| `host_plugin_command` | Call another loaded plugin through the host permission bridge |
| `host_get_runtime_context` | Read generic host runtime context (server/auth/workspace/guest-mode state) |
| `host_namespace_create` / `host_namespace_list` | Host-backed namespace creation and listing |
| `host_namespace_put_object` / `host_namespace_get_object` / `host_namespace_delete_object` | Host-backed namespace object writes, reads, and deletes |
| `host_namespace_list_objects` | Host-backed namespace object metadata listing with optional prefix/limit/offset |
| `host_emit_event` / `host_ws_request` / `host_get_timestamp` / `host_get_now` | Eventing and utility functions |

## Permission enforcement

Permissions are checked in host functions via `HostContext.permission_checker`.

- If no checker is configured, host calls are denied.
- `HostContext::with_fs()` defaults to `DenyAllPermissionChecker`.
- Loader updates runtime `plugin_id` from the guest manifest `id`, so checks
  are keyed to the canonical plugin ID.

Provided checkers:

- `DenyAllPermissionChecker` — denies every request
- `FrontmatterPermissionChecker` — reads root frontmatter `plugins` config and
  normalizes workspace file targets to workspace-relative paths before
  delegating to `diaryx_core::plugin::permissions::check_permission`

Storage keys are plugin-scoped in host functions (`{plugin_id}:{key}`), so one
plugin cannot read another plugin's storage by key collision.

`plugin_storage` is treated as sandbox-safe and defaults to allowed when no
explicit rule exists. File, HTTP, and cross-plugin command permissions still
flow through the configured checker.

Native and browser hosts now both support temporary `host_request_file`
payloads for plugin commands initiated from UI file-picking flows, so guest
plugins can rely on the same `{ file_key } -> raw bytes` contract on both
platform families.

On iOS, the host also lowers Wasmtime's linear-memory reservation from the
default 4 GiB to a mobile-safe size before instantiating plugins. That avoids
`mmap failed to reserve 0x100000000 bytes` failures in TestFlight/App Store
builds while still using Pulley's non-JIT execution path.
