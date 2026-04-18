//! Dev-only HTTP IPC listener for driving a running Tauri instance
//! without the native webview.
//!
//! Compiled out entirely unless the `dev-ipc` Cargo feature is enabled.
//! Every endpoint except `/health` requires an `X-Diaryx-Dev-Token`
//! header matching a per-run random token written to the discovery file.
//! The `/eval` endpoint additionally requires `DIARYX_DEV_IPC_EVAL=1`
//! since it runs arbitrary JS in the main webview.
//!
//! Discovery file is written to `apps/tauri/.dev-ipc.json` (build-time
//! manifest path) and to `<app_data_dir>/.dev-ipc.json` as fallback.
//! Both are deleted on graceful shutdown via the returned guard.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tiny_http::{Header, Method, Response, Server};
use uuid::Uuid;

const DISCOVERY_FILENAME: &str = ".dev-ipc.json";
const TOKEN_HEADER: &str = "X-Diaryx-Dev-Token";

#[derive(Serialize)]
struct Discovery {
    port: u16,
    token: String,
    pid: u32,
}

#[derive(Deserialize)]
struct EmitBody {
    event: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct EvalBody {
    js: String,
    #[serde(default)]
    window: Option<String>,
}

pub struct DevIpcGuard {
    paths: Vec<PathBuf>,
    server: Arc<Server>,
}

impl Drop for DevIpcGuard {
    fn drop(&mut self) {
        self.server.unblock();
        for p in &self.paths {
            let _ = fs::remove_file(p);
        }
    }
}

/// Start the listener. Returns `None` on bind failure.
pub fn start<R: Runtime>(app: &AppHandle<R>) -> Option<DevIpcGuard> {
    let server = match Server::http("127.0.0.1:0") {
        Ok(s) => s,
        Err(err) => {
            log::warn!("[dev-ipc] failed to bind 127.0.0.1:0: {err}");
            return None;
        }
    };
    let port = server.server_addr().to_ip().map(|a| a.port())?;
    let token = Uuid::new_v4().simple().to_string();

    let body = serde_json::to_string_pretty(&Discovery {
        port,
        token: token.clone(),
        pid: std::process::id(),
    })
    .unwrap_or_default();

    let mut paths = Vec::new();

    let repo_path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../", ".dev-ipc.json"));
    if let Some(parent) = repo_path.parent()
        && parent.exists()
        && fs::write(&repo_path, &body).is_ok()
    {
        log::info!("[dev-ipc] discovery: {}", repo_path.display());
        paths.push(repo_path);
    }

    if let Ok(data_dir) = app.path().app_data_dir() {
        let fallback = data_dir.join(DISCOVERY_FILENAME);
        if fs::create_dir_all(&data_dir).is_ok() && fs::write(&fallback, &body).is_ok() {
            log::info!("[dev-ipc] discovery (fallback): {}", fallback.display());
            paths.push(fallback);
        }
    }

    log::info!(
        "[dev-ipc] listening on http://127.0.0.1:{port} (token prefix: {}…)",
        &token[..token.len().min(8)]
    );

    let server = Arc::new(server);
    let thread_server = Arc::clone(&server);
    let app_handle = app.clone();
    let token_arc = Arc::new(token);
    thread::Builder::new()
        .name("diaryx-dev-ipc".into())
        .spawn(move || serve_loop(thread_server, app_handle, token_arc))
        .ok();

    Some(DevIpcGuard { paths, server })
}

fn serve_loop<R: Runtime>(server: Arc<Server>, app: AppHandle<R>, token: Arc<String>) {
    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (url, String::new()),
        };
        let method = req.method().clone();

        if path == "/health" && method == Method::Get {
            let _ = req.respond(json_response(
                200,
                &serde_json::json!({
                    "ok": true,
                    "version": env!("CARGO_PKG_VERSION"),
                    "pid": std::process::id(),
                }),
            ));
            continue;
        }

        if !check_token(&req, token.as_str()) {
            let _ = req.respond(json_response(
                401,
                &serde_json::json!({"error": "unauthorized"}),
            ));
            continue;
        }

        let resp = match (method, path.as_str()) {
            (Method::Post, "/execute") => handle_execute(&mut req, &app),
            (Method::Post, "/emit") => handle_emit(&mut req, &app),
            (Method::Post, "/eval") => handle_eval(&mut req, &app),
            (Method::Get, "/state") => handle_state(&app),
            (Method::Get, "/log") => handle_log(&app, &query),
            (Method::Get, "/screenshot") => handle_screenshot(&query),
            _ => json_response(404, &serde_json::json!({"error": "not found"})),
        };
        let _ = req.respond(resp);
    }
}

fn check_token(req: &tiny_http::Request, expected: &str) -> bool {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(TOKEN_HEADER))
        .map(|h| constant_time_eq(h.value.as_str().as_bytes(), expected.as_bytes()))
        .unwrap_or(false)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn read_body(req: &mut tiny_http::Request) -> Result<serde_json::Value, String> {
    let mut buf = String::new();
    req.as_reader()
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&buf).map_err(|e| e.to_string())
}

fn handle_execute<R: Runtime>(
    req: &mut tiny_http::Request,
    app: &AppHandle<R>,
) -> Response<Cursor<Vec<u8>>> {
    let body = match read_body(req) {
        Ok(v) => v,
        Err(e) => {
            return json_response(400, &serde_json::json!({"error": format!("body: {e}")}));
        }
    };
    // Body is the Command JSON itself, shaped `{"type":"X","params":{...}}`.
    let command_json = body.to_string();
    let app = app.clone();
    let result = tauri::async_runtime::block_on(crate::commands::execute(app, command_json));
    match result {
        Ok(response_json) => {
            let parsed: serde_json::Value =
                serde_json::from_str(&response_json).unwrap_or(serde_json::Value::Null);
            json_response(200, &parsed)
        }
        Err(e) => json_response(
            500,
            &serde_json::json!({
                "error": "execute failed",
                "kind": e.kind,
                "message": e.message,
            }),
        ),
    }
}

fn handle_emit<R: Runtime>(
    req: &mut tiny_http::Request,
    app: &AppHandle<R>,
) -> Response<Cursor<Vec<u8>>> {
    let body = match read_body(req) {
        Ok(v) => v,
        Err(e) => {
            return json_response(400, &serde_json::json!({"error": format!("body: {e}")}));
        }
    };
    let emit: EmitBody = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return json_response(400, &serde_json::json!({"error": format!("payload: {e}")}));
        }
    };
    match app.emit(&emit.event, emit.payload) {
        Ok(()) => json_response(200, &serde_json::json!({"emitted": emit.event})),
        Err(e) => json_response(500, &serde_json::json!({"error": format!("emit: {e}")})),
    }
}

fn handle_eval<R: Runtime>(
    req: &mut tiny_http::Request,
    app: &AppHandle<R>,
) -> Response<Cursor<Vec<u8>>> {
    if std::env::var("DIARYX_DEV_IPC_EVAL").ok().as_deref() != Some("1") {
        return json_response(
            403,
            &serde_json::json!({
                "error": "eval disabled; set DIARYX_DEV_IPC_EVAL=1 to enable",
            }),
        );
    }
    let body = match read_body(req) {
        Ok(v) => v,
        Err(e) => {
            return json_response(400, &serde_json::json!({"error": format!("body: {e}")}));
        }
    };
    let eval: EvalBody = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(e) => {
            return json_response(400, &serde_json::json!({"error": format!("payload: {e}")}));
        }
    };
    let label = eval.window.as_deref().unwrap_or("main");
    let window = match app.get_webview_window(label) {
        Some(w) => w,
        None => {
            return json_response(
                404,
                &serde_json::json!({"error": format!("no webview window named {label:?}")}),
            );
        }
    };
    match window.eval(&eval.js) {
        Ok(()) => json_response(200, &serde_json::json!({"evaluated": label})),
        Err(e) => json_response(500, &serde_json::json!({"error": format!("eval: {e}")})),
    }
}

fn handle_state<R: Runtime>(app: &AppHandle<R>) -> Response<Cursor<Vec<u8>>> {
    use crate::commands::{AppState, GuestModeState};
    let workspace = app
        .state::<AppState>()
        .workspace_path
        .lock()
        .ok()
        .and_then(|g| g.clone().map(|p| p.display().to_string()));
    let guest = app
        .state::<GuestModeState>()
        .active
        .lock()
        .map(|g| *g)
        .unwrap_or(false);
    let data_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    json_response(
        200,
        &serde_json::json!({
            "workspace_path": workspace,
            "is_guest_mode": guest,
            "app_data_dir": data_dir,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
    )
}

fn handle_log<R: Runtime>(app: &AppHandle<R>, query: &str) -> Response<Cursor<Vec<u8>>> {
    let q = parse_query(query);
    let tail: Option<usize> = q.get("tail").and_then(|v| v.parse().ok());
    let previous = q
        .get("previous")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let data_dir = match app.path().app_data_dir() {
        Ok(p) => p,
        Err(e) => {
            return json_response(
                500,
                &serde_json::json!({"error": format!("app_data_dir: {e}")}),
            );
        }
    };
    let (_, log_file) = crate::logging::log_paths(&data_dir);
    let path = if previous {
        log_file.with_extension("previous.log")
    } else {
        log_file
    };
    let content = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return json_response(
                404,
                &serde_json::json!({
                    "error": format!("read {}: {e}", path.display()),
                }),
            );
        }
    };
    let content = match tail {
        Some(n) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        }
        None => content,
    };
    json_response(
        200,
        &serde_json::json!({
            "path": path.display().to_string(),
            "content": content,
        }),
    )
}

fn handle_screenshot(query: &str) -> Response<Cursor<Vec<u8>>> {
    use base64::Engine as _;
    let q = parse_query(query);
    let format = q.get("format").map(String::as_str).unwrap_or("png");
    let pid_filter: u32 = q
        .get("pid")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(std::process::id);

    let png = match capture_own_window_png(pid_filter) {
        Ok(bytes) => bytes,
        Err(e) => {
            return json_response(
                500,
                &serde_json::json!({"error": format!("screenshot: {e}")}),
            );
        }
    };

    match format {
        "json" => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
            json_response(
                200,
                &serde_json::json!({
                    "mime": "image/png",
                    "bytes": png.len(),
                    "data_base64": b64,
                }),
            )
        }
        _ => {
            let hdr: Header = "Content-Type: image/png".parse().expect("static header");
            Response::from_data(png)
                .with_header(hdr)
                .with_status_code(200)
        }
    }
}

fn capture_own_window_png(pid: u32) -> Result<Vec<u8>, String> {
    use image::ImageFormat;
    use xcap::Window;

    let all = Window::all().map_err(|e| format!("enumerate: {e}"))?;
    let candidate = all
        .into_iter()
        .find(|w| {
            let same_pid = w.pid().map(|p| p == pid).unwrap_or(false);
            let not_min = !w.is_minimized().unwrap_or(true);
            same_pid && not_min
        })
        .ok_or_else(|| format!("no on-screen window found for pid {pid}"))?;

    let img = candidate
        .capture_image()
        .map_err(|e| format!("capture: {e}"))?;

    let mut bytes = Vec::with_capacity(256 * 1024);
    img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|e| format!("encode png: {e}"))?;
    Ok(bytes)
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?.to_string();
            let v = it.next().unwrap_or("").to_string();
            Some((k, v))
        })
        .collect()
}

fn json_response(code: u16, value: &serde_json::Value) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let hdr: Header = "Content-Type: application/json"
        .parse()
        .expect("static header");
    Response::from_data(body)
        .with_header(hdr)
        .with_status_code(code)
}
