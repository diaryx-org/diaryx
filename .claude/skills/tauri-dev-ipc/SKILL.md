---
name: tauri-dev-ipc
description: Drive a running Diaryx Tauri dev build over its debug HTTP IPC — execute workspace commands, read file-backed logs, emit frontend events, inspect state, or eval JS in the webview. Use when asked to "run the tauri app", "test the app from the outside", "read tauri logs", "send a command to diaryx", "check what the app is doing", or any scripted control of the running Tauri desktop build. Does NOT work for release builds (the listener is compiled out) or for the web/wasm frontend — use Playwright MCP against `apps/web` for those.
---

# tauri-dev-ipc

Programmatic control of a running `bun run tauri:dev-ipc` session. The listener lives in `apps/tauri/src-tauri/src/dev_ipc.rs` and binds on `127.0.0.1:<ephemeral>`. Every endpoint except `/health` requires a per-run random token from the discovery file.

## 1. Is the app running?

The authoritative sign: `apps/tauri/.dev-ipc.json` exists. It's deleted on graceful shutdown. Read it first every session — the port and token change each run.

```bash
cat apps/tauri/.dev-ipc.json
# => {"port": <u16>, "token": "<uuid>", "pid": <u32>}
```

If it's missing:

- App isn't running, OR
- App was built without the `dev-ipc` Cargo feature, OR
- App crashed ungracefully and left no file.

Tell the user: "Start the app with `cd apps/tauri && bun run tauri:dev-ipc`". Don't guess the port.

Second sanity check once you have the port/token: `GET /health` returns `{"ok":true,"pid":<n>,"version":"<v>"}` with no token required. If this fails, the listener isn't bound.

## 2. How to call it

Always use the helper — it handles token + content-type + status mapping:

```bash
bash apps/tauri/scripts/dev-ipc.sh <METHOD> <PATH> [curl args]
```

Exit codes: `0` on 2xx, `4` on 4xx, `5` on 5xx, `2` if discovery file missing, `1` otherwise. Body always prints to stdout (JSON on both success and failure), so pipe straight to `python3 -m json.tool` or `jq`.

## 3. Endpoints

| Method | Path       | Auth | Body / Query                                          | Returns                                                |
| ------ | ---------- | ---- | ----------------------------------------------------- | ------------------------------------------------------ |
| GET    | `/health`  | no   | —                                                     | `{ok, version, pid}`                                   |
| GET    | `/state`   | yes  | —                                                     | `{workspace_path, is_guest_mode, app_data_dir, pid, version}` |
| GET    | `/log`     | yes  | `?tail=N` (last N lines), `?previous=1` (rotated log) | `{path, content}`                                      |
| POST   | `/execute` | yes  | Command JSON: `{"type":"X","params":{...}}`          | Response JSON (shape varies per variant)               |
| POST   | `/emit`    | yes  | `{"event": "name", "payload": any}`                   | `{emitted: name}`                                      |
| POST   | `/eval`    | yes† | `{"js": "...", "window": "main"}` (window optional)   | `{evaluated: <label>}`                                 |
| GET    | `/screenshot` | yes | `?format=png` (default, binary) \| `?format=json` (base64) \| `?pid=<n>` | PNG of the app's native window |

† `/eval` is extra-gated by `DIARYX_DEV_IPC_EVAL=1` in the app's environment. If disabled, returns 403 with a message saying how to enable. Use sparingly — it's an arbitrary-JS escape hatch.

## 4. The Command payload

`Command` is a serde-tagged enum defined in `crates/diaryx_core/src/command.rs`:

```rust
#[serde(tag = "type", content = "params")]
pub enum Command { GetEntry { path: String }, ... }
```

Wire shape is always `{"type": "<VariantName>", "params": {...variant fields...}}`. To find the exact params for any variant, grep `crates/diaryx_core/src/command.rs` — don't guess field names.

Common recipes:

```bash
# Workspace tree (depth 1 from root index)
bash apps/tauri/scripts/dev-ipc.sh POST /execute --data '{
  "type":"GetWorkspaceTree",
  "params":{"path":"README.md","depth":1,"audience":null}
}'

# Read an entry
bash apps/tauri/scripts/dev-ipc.sh POST /execute --data '{
  "type":"GetEntry",
  "params":{"path":"README.md"}
}'

# Search
bash apps/tauri/scripts/dev-ipc.sh POST /execute --data '{
  "type":"SearchWorkspace",
  "params":{"query":"hello","scope":"Content"}
}'

# Frontmatter read
bash apps/tauri/scripts/dev-ipc.sh POST /execute --data '{
  "type":"GetFrontmatter",
  "params":{"path":"README.md"}
}'
```

Paths are workspace-relative (no leading `/`), resolved against `state.workspace_path` from `GET /state`.

## 5. Response shapes

`/execute` returns whatever `commands::execute` returns. The Response enum is also serde-tagged; some variants carry data via `{"type":"Entry","<fields>"}`, others wrap the payload in `{"data": ...}`. If the shape is unclear for a variant, grep `crates/diaryx_core/src/command.rs` near the `Response` enum.

Errors from Diaryx surface as 500s with `{error, kind, message}`. Transport-level errors (bad JSON, missing endpoint, auth) come back as 400/401/404 with `{error}`.

## 6. Reading logs

The backend mirrors everything to `<app_data_dir>/logs/diaryx.log`. For tight loops, poll `/log?tail=50` after each action. Don't fetch the whole log — it can be multi-MB and gets rotated at 2 MiB into `diaryx.previous.log` (`?previous=1` to read the rotated one).

After invoking a command, the log is where you verify what actually happened on the Rust side — look for `[CommandHandler]` and module-path prefixes like `[diaryx_tauri_lib::commands]`.

## 7. Driving the UI

You cannot hook up Playwright to the native Tauri webview. Three tools instead:

- `POST /emit` — fires a `tauri::Emitter` event the frontend can listen for. Same mechanism the Rust side uses to notify Svelte. Useful if you want to trigger a specific frontend reaction.
- `POST /eval` — runs arbitrary JS in the main webview. Needs `DIARYX_DEV_IPC_EVAL=1`. Use for clicks, typing, scrolling, reading DOM state, invoking `globalThis.__diaryx_*` bridges. **No return value** back through HTTP — results must be observed via logs, events, follow-up `/execute` / `/state` calls, or `/screenshot`.
- `GET /screenshot` — captures the native window via `xcap`. Default returns raw `image/png`. Use after any UI action to verify what rendered. Save and read:

  ```bash
  bash apps/tauri/scripts/dev-ipc.sh GET /screenshot > /tmp/diaryx.png
  # then Read /tmp/diaryx.png
  ```

  The first screenshot on macOS triggers a one-time Screen Recording permission prompt. Subsequent calls are silent. `?format=json` returns `{mime, bytes, data_base64}` if binary handling is inconvenient.

Common click-and-verify loop:

```bash
# Click something
bash apps/tauri/scripts/dev-ipc.sh POST /eval --data '{"js":"document.querySelector(\"[data-test=save]\").click()"}'
# See what happened
bash apps/tauri/scripts/dev-ipc.sh GET "/log?tail=20"
bash apps/tauri/scripts/dev-ipc.sh GET /screenshot > /tmp/shot.png
```

For heavier UI work, prefer driving the web frontend at `apps/web` with Playwright MCP — it's the same Svelte code.

## 8. Lifecycle notes

- Source changes to `dev_ipc.rs` or `commands.rs` need a full `bun run tauri:dev-ipc` restart. Vite HMR only touches the frontend.
- Each restart generates a fresh token + port. Re-read `.dev-ipc.json` after any restart.
- If requests start 401-ing with the correct token, the app restarted and the discovery file is stale. Re-read it.
- The listener is compiled out entirely in release builds. Any instructions here apply only to debug dev builds.

## 9. When NOT to use this skill

- User is debugging the web/wasm build → use Playwright MCP against `apps/web`.
- User is asking about release behavior → IPC isn't present in release builds.
- Task is pure code inspection — read the source, don't call the app.
