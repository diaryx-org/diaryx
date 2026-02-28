# diaryx_extism

Extism-based third-party plugin runtime for Diaryx.

Loads WebAssembly plugin modules via the [Extism](https://extism.org/) runtime and adapts them to the `diaryx_core` `Plugin`, `WorkspacePlugin`, and `FilePlugin` traits. Guest plugins communicate with the host through a JSON protocol.

## Plugin directory structure

```
~/.diaryx/plugins/
  my-plugin/
    plugin.wasm      # The WASM module
    manifest.json    # Optional cached manifest
    config.json      # Plugin config (created at runtime)
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

## Host functions available to guests

| Function | Input | Output | Description |
|----------|-------|--------|-------------|
| `host_log` | `{level, message}` | `""` | Log via `log` crate |
| `host_read_file` | `{path}` | file content | Read a workspace file |
| `host_list_files` | `{prefix}` | `string[]` | List files under prefix |
| `host_file_exists` | `{path}` | `bool` | Check file existence |
