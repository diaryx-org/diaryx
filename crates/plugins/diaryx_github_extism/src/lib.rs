use std::cell::RefCell;
use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use diaryx_plugin_sdk::host;
use diaryx_plugin_sdk::host::http::HttpRequestOptions;
use diaryx_plugin_sdk::prelude::*;
use extism_pdk::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

thread_local! {
    static CONFIG: RefCell<Option<GithubConfig>> = const { RefCell::new(None) };
}

const CONFIG_STORAGE_KEY: &str = "github_config";
const ACCESS_TOKEN_SECRET_KEY: &str = "github_access_token";
const HISTORY_COMPONENT_ID: &str = "github.history";
const API_BASE: &str = "https://api.github.com";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const HTTP_TIMEOUT_MS: u64 = 30_000;
const MAX_SNAPSHOT_BYTES: usize = 20 * 1024 * 1024;
const EXCLUDED_PATH_PARTS: &[&str] = &[
    ".git",
    ".jj",
    ".svn",
    "target",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".turbo",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GithubConfig {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    repo_owner: String,
    #[serde(default)]
    repo_name: String,
    #[serde(default = "default_branch")]
    branch: String,
    #[serde(default = "default_workspace_root")]
    workspace_root: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    connected_login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceSnapshot {
    version: u32,
    workspace_id: String,
    workspace_name: String,
    generated_at: String,
    files: Vec<WorkspaceSnapshotFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceSnapshotFile {
    path: String,
    content_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommitSummary {
    sha: String,
    message: String,
    author: String,
    date: String,
    html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeWorkspaceContext {
    local_id: String,
    name: String,
    remote_id: Option<String>,
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_workspace_root() -> String {
    "workspaces".to_string()
}

fn default_config() -> GithubConfig {
    GithubConfig {
        branch: default_branch(),
        workspace_root: default_workspace_root(),
        ..GithubConfig::default()
    }
}

fn with_config<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&GithubConfig) -> Result<R, String>,
{
    CONFIG.with(|state| {
        let borrow = state.borrow();
        let config = borrow
            .as_ref()
            .ok_or("GitHub plugin not configured".to_string())?;
        f(config)
    })
}

fn with_config_mut<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut GithubConfig) -> Result<R, String>,
{
    CONFIG.with(|state| {
        let mut borrow = state.borrow_mut();
        let config = borrow
            .as_mut()
            .ok_or("GitHub plugin not configured".to_string())?;
        f(config)
    })
}

fn set_runtime_config(config: GithubConfig) {
    CONFIG.with(|state| *state.borrow_mut() = Some(config));
}

fn current_config_or_default() -> GithubConfig {
    CONFIG.with(|state| state.borrow().clone().unwrap_or_else(default_config))
}

fn persist_config(config: &GithubConfig) -> Result<(), String> {
    if config.access_token.trim().is_empty() {
        host::secrets::delete(ACCESS_TOKEN_SECRET_KEY)?;
    } else {
        host::secrets::set(ACCESS_TOKEN_SECRET_KEY, &config.access_token)?;
    }

    let mut stored = config.clone();
    stored.access_token.clear();
    let bytes = serde_json::to_vec(&stored).map_err(|e| format!("serialize config: {e}"))?;
    host::storage::set(CONFIG_STORAGE_KEY, &bytes)
}

fn load_persisted_config() -> Result<Option<GithubConfig>, String> {
    let Some(bytes) = host::storage::get(CONFIG_STORAGE_KEY)? else {
        return Ok(None);
    };

    let mut config: GithubConfig =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse config: {e}"))?;
    if let Some(token) = host::secrets::get(ACCESS_TOKEN_SECRET_KEY)? {
        config.access_token = token;
    }
    if config.branch.trim().is_empty() {
        config.branch = default_branch();
    }
    if config.workspace_root.trim().is_empty() {
        config.workspace_root = default_workspace_root();
    }
    Ok(Some(config))
}

fn merge_with_current_config(mut incoming: GithubConfig) -> GithubConfig {
    let current = current_config_or_default();
    if incoming.client_id.trim().is_empty() {
        incoming.client_id = current.client_id;
    }
    if incoming.repo_owner.trim().is_empty() {
        incoming.repo_owner = current.repo_owner;
    }
    if incoming.repo_name.trim().is_empty() {
        incoming.repo_name = current.repo_name;
    }
    if incoming.branch.trim().is_empty() {
        incoming.branch = current.branch;
    }
    if incoming.workspace_root.trim().is_empty() {
        incoming.workspace_root = current.workspace_root;
    }
    if incoming.access_token.trim().is_empty() {
        incoming.access_token = current.access_token;
    }
    if incoming.connected_login.trim().is_empty() {
        incoming.connected_login = current.connected_login;
    }
    incoming
}

fn view_config(config: &GithubConfig) -> JsonValue {
    json!({
        "client_id": config.client_id,
        "repo_owner": config.repo_owner,
        "repo_name": config.repo_name,
        "branch": config.branch,
        "workspace_root": config.workspace_root,
        "access_token": "",
        "connected": !config.access_token.trim().is_empty(),
        "connected_login": config.connected_login,
    })
}

fn http_headers(token: Option<&str>) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert(
        "Accept".to_string(),
        "application/vnd.github+json".to_string(),
    );
    headers.insert(
        "User-Agent".to_string(),
        format!("Diaryx-GitHub-Plugin/{}", env!("CARGO_PKG_VERSION")),
    );
    headers.insert("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string());
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
    }
    headers
}

fn http_request(
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<host::http::HttpResponse, String> {
    host::http::request_with_options(
        method,
        url,
        headers,
        body,
        HttpRequestOptions {
            timeout_ms: Some(HTTP_TIMEOUT_MS),
        },
    )
}

fn validate_repo_config(config: &GithubConfig) -> Result<(), String> {
    if config.repo_owner.trim().is_empty() || config.repo_name.trim().is_empty() {
        return Err("Set a GitHub repository owner and name first.".to_string());
    }
    if config.access_token.trim().is_empty() {
        return Err("Connect GitHub or paste a personal access token first.".to_string());
    }
    Ok(())
}

fn uri_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}

fn sanitize_remote_id(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else if ch.is_whitespace() || ch == '/' {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').trim();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed.to_string()
    }
}

fn snapshot_path(config: &GithubConfig, remote_id: &str) -> String {
    let root = config.workspace_root.trim().trim_matches('/');
    if root.is_empty() {
        format!("{}.json", sanitize_remote_id(remote_id))
    } else {
        format!("{}/{}.json", root, sanitize_remote_id(remote_id))
    }
}

fn repo_api_path(config: &GithubConfig, suffix: &str) -> String {
    let base = format!(
        "{API_BASE}/repos/{}/{}",
        uri_encode(config.repo_owner.trim()),
        uri_encode(config.repo_name.trim()),
    );
    let suffix = suffix.trim_start_matches('/');
    if suffix.is_empty() {
        base
    } else {
        format!("{base}/{suffix}")
    }
}

fn fetch_current_user_login(_config: &GithubConfig, token: &str) -> Result<String, String> {
    let headers = http_headers(Some(token));
    let resp = http_request("GET", &format!("{API_BASE}/user"), &headers, None)?;
    if resp.status != 200 {
        return Err(format!(
            "GitHub auth check failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse current user: {e}"))?;
    parsed
        .get("login")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "GitHub user response did not include a login".to_string())
}

fn exchange_token(
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<String, String> {
    let body = format!(
        "client_id={}&code={}&redirect_uri={}&code_verifier={}",
        uri_encode(client_id),
        uri_encode(code),
        uri_encode(redirect_uri),
        uri_encode(code_verifier),
    );
    let mut headers = HashMap::new();
    headers.insert("Accept".to_string(), "application/json".to_string());
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );
    headers.insert(
        "User-Agent".to_string(),
        format!("Diaryx-GitHub-Plugin/{}", env!("CARGO_PKG_VERSION")),
    );
    let resp = http_request("POST", TOKEN_URL, &headers, Some(&body))?;
    if resp.status != 200 {
        return Err(format!(
            "GitHub token exchange failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse token response: {e}"))?;
    if let Some(error) = parsed.get("error").and_then(|value| value.as_str()) {
        return Err(format!(
            "GitHub token exchange failed: {}",
            parsed
                .get("error_description")
                .and_then(|value| value.as_str())
                .unwrap_or(error)
        ));
    }
    parsed
        .get("access_token")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "GitHub token response did not include an access token".to_string())
}

fn current_workspace_context(plugin_id: &str) -> Option<RuntimeWorkspaceContext> {
    let runtime = host::context::get().ok()?;
    let current = runtime.get("current_workspace")?.as_object()?;
    let local_id = current.get("local_id")?.as_str()?.to_string();
    let name = current
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("Workspace")
        .to_string();

    let remote_from_links = current
        .get("provider_links")
        .and_then(|value| value.as_array())
        .and_then(|links| {
            links.iter().find_map(|link| {
                let link_obj = link.as_object()?;
                if link_obj.get("plugin_id")?.as_str()? != plugin_id {
                    return None;
                }
                link_obj
                    .get("remote_workspace_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
        });

    let remote_from_metadata = current
        .get("plugin_metadata")
        .and_then(|value| value.as_object())
        .and_then(|plugins| plugins.get(plugin_id))
        .and_then(|value| value.as_object())
        .and_then(|metadata| {
            metadata
                .get("remoteWorkspaceId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| {
                    metadata
                        .get("serverId")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
        });

    Some(RuntimeWorkspaceContext {
        local_id,
        name,
        remote_id: remote_from_links.or(remote_from_metadata),
    })
}

fn should_sync_path(path: &str) -> bool {
    let normalized = path.trim_matches('/');
    if normalized.is_empty() || normalized.starts_with(".diaryx/") {
        return false;
    }

    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.iter().any(|part| EXCLUDED_PATH_PARTS.contains(part)) {
        return false;
    }
    if parts
        .iter()
        .any(|part| part.starts_with('.') && *part != ".well-known")
    {
        return false;
    }

    true
}

fn build_workspace_snapshot(
    workspace_id: &str,
    workspace_name: &str,
) -> Result<WorkspaceSnapshot, String> {
    let files = host::fs::list_files("")?;
    let mut total_bytes = 0usize;
    let snapshot_files = files
        .into_iter()
        .filter(|path| should_sync_path(path))
        .map(|path| {
            let bytes = host::fs::read_binary(&path)?;
            total_bytes = total_bytes.saturating_add(bytes.len());
            if total_bytes > MAX_SNAPSHOT_BYTES {
                return Err(format!(
                    "Workspace snapshot is too large for GitHub sync (>{} MiB). Use a smaller workspace root or exclude repo/build directories.",
                    MAX_SNAPSHOT_BYTES / 1024 / 1024
                ));
            }
            Ok(WorkspaceSnapshotFile {
                path,
                content_base64: BASE64.encode(bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(WorkspaceSnapshot {
        version: 1,
        workspace_id: workspace_id.to_string(),
        workspace_name: workspace_name.to_string(),
        generated_at: host::time::now_rfc3339()?,
        files: snapshot_files,
    })
}

fn fetch_existing_sha(config: &GithubConfig, remote_path: &str) -> Result<Option<String>, String> {
    let url = format!(
        "{}?ref={}",
        repo_api_path(config, &format!("contents/{}", uri_encode(remote_path))),
        uri_encode(config.branch.trim())
    );
    let headers = http_headers(Some(&config.access_token));
    let resp = http_request("GET", &url, &headers, None)?;
    if resp.status == 404 {
        return Ok(None);
    }
    if resp.status != 200 {
        return Err(format!(
            "GitHub contents lookup failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse contents lookup: {e}"))?;
    Ok(parsed
        .get("sha")
        .and_then(|value| value.as_str())
        .map(str::to_string))
}

fn upload_snapshot(
    config: &GithubConfig,
    remote_id: &str,
    snapshot: &WorkspaceSnapshot,
    commit_message: &str,
) -> Result<bool, String> {
    let remote_path = snapshot_path(config, remote_id);
    let existing_sha = fetch_existing_sha(config, &remote_path)?;
    let payload = serde_json::to_vec(snapshot).map_err(|e| format!("serialize snapshot: {e}"))?;
    let mut body = json!({
        "message": commit_message,
        "content": BASE64.encode(payload),
        "branch": config.branch,
    });
    if let Some(sha) = existing_sha.clone() {
        body["sha"] = JsonValue::String(sha);
    }

    let mut headers = http_headers(Some(&config.access_token));
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    let url = repo_api_path(config, &format!("contents/{}", uri_encode(&remote_path)));
    let resp = http_request("PUT", &url, &headers, Some(&body.to_string()))?;
    if resp.status == 200 || resp.status == 201 {
        Ok(existing_sha.is_none())
    } else {
        Err(format!(
            "GitHub snapshot upload failed ({}): {}",
            resp.status, resp.body
        ))
    }
}

fn list_remote_workspaces(config: &GithubConfig) -> Result<Vec<JsonValue>, String> {
    let root = config.workspace_root.trim().trim_matches('/');
    let suffix = if root.is_empty() {
        format!("contents?ref={}", uri_encode(config.branch.trim()))
    } else {
        format!(
            "contents/{}?ref={}",
            uri_encode(root),
            uri_encode(config.branch.trim())
        )
    };
    let url = repo_api_path(config, &suffix);
    let headers = http_headers(Some(&config.access_token));
    let resp = http_request("GET", &url, &headers, None)?;
    if resp.status == 404 {
        return Ok(Vec::new());
    }
    if resp.status != 200 {
        return Err(format!(
            "GitHub workspace listing failed ({}): {}",
            resp.status, resp.body
        ));
    }

    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse workspace listing: {e}"))?;
    let items = parsed.as_array().cloned().unwrap_or_default();
    Ok(items
        .into_iter()
        .filter_map(|item| {
            let name = item.get("name")?.as_str()?;
            if !name.ends_with(".json") {
                return None;
            }
            let id = name.trim_end_matches(".json").to_string();
            Some(json!({
                "id": id,
                "name": id,
            }))
        })
        .collect())
}

fn fetch_snapshot(config: &GithubConfig, remote_id: &str) -> Result<WorkspaceSnapshot, String> {
    let remote_path = snapshot_path(config, remote_id);
    let url = format!(
        "{}?ref={}",
        repo_api_path(config, &format!("contents/{}", uri_encode(&remote_path))),
        uri_encode(config.branch.trim())
    );
    let headers = http_headers(Some(&config.access_token));
    let resp = http_request("GET", &url, &headers, None)?;
    if resp.status != 200 {
        return Err(format!(
            "GitHub snapshot download failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse snapshot contents: {e}"))?;
    let content = parsed
        .get("content")
        .and_then(|value| value.as_str())
        .map(|value| value.replace('\n', ""))
        .ok_or_else(|| "GitHub contents response did not include file content".to_string())?;
    let bytes = BASE64
        .decode(content)
        .map_err(|e| format!("decode snapshot content: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse snapshot JSON: {e}"))
}

fn restore_snapshot(snapshot: &WorkspaceSnapshot) -> Result<usize, String> {
    for file in &snapshot.files {
        let bytes = BASE64
            .decode(&file.content_base64)
            .map_err(|e| format!("decode {}: {e}", file.path))?;
        match String::from_utf8(bytes.clone()) {
            Ok(text) => host::fs::write_file(&file.path, &text)?,
            Err(_) => host::fs::write_binary(&file.path, &bytes)?,
        }
    }
    Ok(snapshot.files.len())
}

fn get_commit_history(
    config: &GithubConfig,
    remote_id: &str,
) -> Result<Vec<CommitSummary>, String> {
    let remote_path = snapshot_path(config, remote_id);
    let url = format!(
        "{}?sha={}&path={}&per_page=25",
        repo_api_path(config, "commits"),
        uri_encode(config.branch.trim()),
        uri_encode(&remote_path),
    );
    let headers = http_headers(Some(&config.access_token));
    let resp = http_request("GET", &url, &headers, None)?;
    if resp.status != 200 {
        return Err(format!(
            "GitHub commit history failed ({}): {}",
            resp.status, resp.body
        ));
    }
    let parsed: JsonValue =
        serde_json::from_str(&resp.body).map_err(|e| format!("parse commit history: {e}"))?;
    let commits = parsed.as_array().cloned().unwrap_or_default();
    Ok(commits
        .into_iter()
        .filter_map(|item| {
            let sha = item.get("sha")?.as_str()?.to_string();
            let html_url = item
                .get("html_url")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let commit = item.get("commit")?.as_object()?;
            let author = commit
                .get("author")
                .and_then(|value| value.as_object())
                .cloned()
                .unwrap_or_default();
            Some(CommitSummary {
                sha: sha.chars().take(7).collect(),
                message: commit
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Commit")
                    .lines()
                    .next()
                    .unwrap_or("Commit")
                    .to_string(),
                author: author
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                date: author
                    .get("date")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                html_url,
            })
        })
        .collect())
}

fn get_component_html(component_id: &str) -> Result<String, String> {
    match component_id {
        HISTORY_COMPONENT_ID => Ok(include_str!("history.html").to_string()),
        _ => Err(format!("Unknown GitHub component: {component_id}")),
    }
}

#[plugin_fn]
pub fn manifest(_input: String) -> FnResult<String> {
    let manifest = GuestManifest::new(
        "diaryx.github",
        "GitHub Sync",
        env!("CARGO_PKG_VERSION"),
        "GitHub-backed workspace snapshots and commit history",
        vec!["custom_commands".into()],
    )
    .ui(vec![
        json!({
            "slot": "SettingsTab",
            "id": "github-settings",
            "label": "GitHub",
            "icon": "github",
            "fields": [
                {
                    "type": "Section",
                    "label": "Connection",
                    "description": "Use a GitHub OAuth app client ID from the host or paste a personal access token."
                },
                {
                    "type": "Password",
                    "key": "access_token",
                    "label": "Personal Access Token",
                    "description": "Optional fallback when OAuth is unavailable on the current platform.",
                    "placeholder": "ghp_..."
                },
                {
                    "type": "Button",
                    "label": "Connect GitHub",
                    "command": "BeginOAuth"
                },
                {
                    "type": "Button",
                    "label": "Disconnect",
                    "command": "Disconnect",
                    "variant": "outline"
                },
                {
                    "type": "Section",
                    "label": "Repository",
                    "description": "Workspace snapshots are committed into a single repository as versioned JSON files."
                },
                {
                    "type": "Text",
                    "key": "repo_owner",
                    "label": "Repository Owner",
                    "placeholder": "diaryx-org"
                },
                {
                    "type": "Text",
                    "key": "repo_name",
                    "label": "Repository Name",
                    "placeholder": "workspace-sync"
                },
                {
                    "type": "Text",
                    "key": "branch",
                    "label": "Branch",
                    "placeholder": "main"
                },
                {
                    "type": "Text",
                    "key": "workspace_root",
                    "label": "Workspace Folder",
                    "placeholder": "workspaces"
                },
                {
                    "type": "Button",
                    "label": "Sync Current Workspace",
                    "command": "SyncWorkspace",
                    "variant": "outline"
                }
            ]
        }),
        json!({
            "slot": "SidebarTab",
            "id": "github-history",
            "label": "GitHub",
            "icon": "github",
            "side": "Right",
            "component": {
                "type": "Iframe",
                "component_id": HISTORY_COMPONENT_ID,
            }
        }),
        json!({
            "slot": "WorkspaceProvider",
            "id": "diaryx.github",
            "label": "GitHub",
            "description": "Store workspace snapshots in a GitHub repository"
        })
    ])
    .commands(vec![
        "BeginOAuth".into(),
        "CompleteOAuth".into(),
        "Disconnect".into(),
        "GetProviderStatus".into(),
        "ListRemoteWorkspaces".into(),
        "LinkWorkspace".into(),
        "UnlinkWorkspace".into(),
        "DownloadWorkspace".into(),
        "SyncWorkspace".into(),
        "GetCommitHistory".into(),
        "get_component_html".into(),
    ])
    .requested_permissions(GuestRequestedPermissions {
        defaults: json!({
            "plugin_storage": { "include": ["all"], "exclude": [] },
            "http_requests": {
                "include": ["api.github.com", "github.com"],
                "exclude": []
            },
            "read_files": { "include": ["all"], "exclude": [] },
            "edit_files": { "include": ["all"], "exclude": [] },
            "create_files": { "include": ["all"], "exclude": [] }
        }),
        reasons: [
            ("plugin_storage".to_string(), "Store GitHub configuration and tokens.".to_string()),
            ("http_requests".to_string(), "Call GitHub OAuth, repository contents, and commit history APIs.".to_string()),
            ("read_files".to_string(), "Read workspace files before uploading a snapshot.".to_string()),
            ("edit_files".to_string(), "Restore downloaded snapshot files into the workspace.".to_string()),
            ("create_files".to_string(), "Create files when downloading a linked workspace.".to_string()),
        ]
        .into_iter()
        .collect(),
    });
    Ok(serde_json::to_string(&manifest)?)
}

#[plugin_fn]
pub fn init(_input: String) -> FnResult<String> {
    if let Ok(Some(config)) = load_persisted_config() {
        set_runtime_config(config);
    }
    Ok(String::new())
}

#[plugin_fn]
pub fn shutdown(_input: String) -> FnResult<String> {
    CONFIG.with(|state| *state.borrow_mut() = None);
    Ok(String::new())
}

#[plugin_fn]
pub fn get_config(_input: String) -> FnResult<String> {
    let config = CONFIG.with(|state| state.borrow().clone());
    Ok(serde_json::to_string(&view_config(
        &config.unwrap_or_else(default_config),
    ))?)
}

#[plugin_fn]
pub fn set_config(input: String) -> FnResult<String> {
    let mut config = merge_with_current_config(serde_json::from_str(&input)?);
    if !config.access_token.trim().is_empty() {
        config.connected_login =
            fetch_current_user_login(&config, &config.access_token).map_err(Error::msg)?;
    }
    persist_config(&config).map_err(Error::msg)?;
    set_runtime_config(config);
    Ok(String::new())
}

#[plugin_fn]
pub fn on_event(_input: String) -> FnResult<String> {
    Ok(String::new())
}

#[plugin_fn]
pub fn handle_command(input: String) -> FnResult<String> {
    let req: CommandRequest = serde_json::from_str(&input)?;
    let response = dispatch_command(&req.command, &req.params);
    Ok(serde_json::to_string(&response)?)
}

fn dispatch_command(command: &str, params: &JsonValue) -> CommandResponse {
    let result = match command {
        "get_component_html" => params
            .get("component_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Missing component_id".to_string())
            .and_then(get_component_html)
            .map(JsonValue::String),
        "BeginOAuth" => {
            let config = current_config_or_default();
            let client_id = params
                .get("client_id")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
                .or_else(|| {
                    let fallback = config.client_id.trim();
                    if fallback.is_empty() {
                        None
                    } else {
                        Some(fallback.to_string())
                    }
                })
                .ok_or_else(|| {
                    "GitHub OAuth is not configured for this build. Set a host-managed client ID or use a personal access token.".to_string()
                });

            client_id.and_then(|client_id| {
                let redirect_uri = params
                    .get("redirect_uri")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "Missing redirect_uri".to_string())?;
                let redirect_uri_prefix = params
                    .get("redirect_uri_prefix")
                    .and_then(|value| value.as_str())
                    .unwrap_or(redirect_uri);
                let code_challenge = params
                    .get("code_challenge")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "Missing code_challenge".to_string())?;
                Ok(json!({
                    "host_action": {
                        "type": "open-oauth",
                        "payload": {
                            "url": format!(
                                "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256",
                                uri_encode(&client_id),
                                uri_encode(redirect_uri),
                                uri_encode("repo"),
                                uri_encode(code_challenge),
                            ),
                            "redirect_uri_prefix": redirect_uri_prefix,
                        }
                    },
                    "follow_up": {
                        "command": "CompleteOAuth",
                        "params": {
                            "client_id": client_id,
                            "code_verifier": params.get("code_verifier").cloned().unwrap_or(JsonValue::Null),
                        }
                    }
                }))
            })
        }
        "CompleteOAuth" => {
            let code = params
                .get("code")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "Missing code".to_string());
            let redirect_uri = params
                .get("redirect_uri")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "Missing redirect_uri".to_string());
            let client_id = params
                .get("client_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| {
                    let existing = current_config_or_default().client_id;
                    if existing.trim().is_empty() {
                        None
                    } else {
                        Some(existing)
                    }
                })
                .ok_or_else(|| "Missing client_id".to_string());
            let code_verifier = params
                .get("code_verifier")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "Missing code_verifier".to_string());

            code.and_then(|code| {
                redirect_uri.and_then(|redirect_uri| {
                    client_id.and_then(|client_id| {
                        code_verifier.and_then(|code_verifier| {
                            let token =
                                exchange_token(&client_id, code, redirect_uri, code_verifier)?;
                            let mut config = current_config_or_default();
                            config.client_id = client_id;
                            config.access_token = token.clone();
                            config.connected_login = fetch_current_user_login(&config, &token)?;
                            persist_config(&config)?;
                            set_runtime_config(config.clone());
                            Ok(json!({
                                "message": format!("Connected as {}.", config.connected_login),
                            }))
                        })
                    })
                })
            })
        }
        "Disconnect" => with_config_mut(|config| {
            config.access_token.clear();
            config.connected_login.clear();
            persist_config(config)?;
            Ok(json!({ "message": "Disconnected from GitHub." }))
        }),
        "GetProviderStatus" => with_config(|config| {
            if config.repo_owner.trim().is_empty() || config.repo_name.trim().is_empty() {
                return Ok(json!({
                    "ready": false,
                    "message": "Set a repository owner and repository name.",
                }));
            }
            if config.access_token.trim().is_empty() {
                return Ok(json!({
                    "ready": false,
                    "message": "Connect GitHub or paste a personal access token.",
                }));
            }
            let url = repo_api_path(config, "");
            let resp = http_request("GET", &url, &http_headers(Some(&config.access_token)), None)?;
            if resp.status == 200 {
                Ok(json!({
                    "ready": true,
                    "message": format!("{}/{}", config.repo_owner, config.repo_name),
                }))
            } else {
                Ok(json!({
                    "ready": false,
                    "message": format!("GitHub repo check failed ({}).", resp.status),
                }))
            }
        }),
        "ListRemoteWorkspaces" => with_config(|config| {
            validate_repo_config(config)?;
            Ok(json!({
                "workspaces": list_remote_workspaces(config)?,
            }))
        }),
        "LinkWorkspace" => with_config(|config| {
            validate_repo_config(config)?;
            let local_id = params
                .get("local_workspace_id")
                .and_then(|value| value.as_str())
                .unwrap_or("workspace");
            let name = params
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("Workspace");
            let remote_id = params
                .get("remote_id")
                .and_then(|value| value.as_str())
                .map(sanitize_remote_id)
                .unwrap_or_else(|| sanitize_remote_id(local_id));
            let snapshot = build_workspace_snapshot(local_id, name)?;
            let created_remote = upload_snapshot(
                config,
                &remote_id,
                &snapshot,
                &format!("Sync workspace {name} ({remote_id})"),
            )?;
            Ok(json!({
                "remote_id": remote_id,
                "created_remote": created_remote,
                "snapshot_uploaded": true,
            }))
        }),
        "UnlinkWorkspace" => Ok(json!({ "message": "Workspace unlinked." })),
        "DownloadWorkspace" => with_config(|config| {
            validate_repo_config(config)?;
            let runtime = current_workspace_context("diaryx.github");
            let remote_id = params
                .get("remote_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| runtime.and_then(|value| value.remote_id))
                .ok_or_else(|| "No GitHub remote workspace is linked.".to_string())?;
            let snapshot = fetch_snapshot(config, &remote_id)?;
            let files_imported = restore_snapshot(&snapshot)?;
            Ok(json!({
                "files_imported": files_imported,
            }))
        }),
        "SyncWorkspace" => with_config(|config| {
            validate_repo_config(config)?;
            let runtime = current_workspace_context("diaryx.github")
                .ok_or_else(|| "No active workspace context is available.".to_string())?;
            let remote_id = params
                .get("remote_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or(runtime.remote_id)
                .unwrap_or_else(|| sanitize_remote_id(&runtime.local_id));
            let snapshot = build_workspace_snapshot(&runtime.local_id, &runtime.name)?;
            upload_snapshot(
                config,
                &remote_id,
                &snapshot,
                &format!("Sync workspace {} ({remote_id})", runtime.name),
            )?;
            Ok(json!({
                "message": format!("Synced {} to GitHub.", runtime.name),
                "remote_id": remote_id,
            }))
        }),
        "GetCommitHistory" => with_config(|config| {
            validate_repo_config(config)?;
            let runtime = current_workspace_context("diaryx.github");
            let remote_id = params
                .get("remote_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| runtime.and_then(|value| value.remote_id))
                .ok_or_else(|| "No GitHub remote workspace is linked.".to_string())?;
            Ok(json!({
                "remote_id": remote_id,
                "repo": format!("{}/{}", config.repo_owner, config.repo_name),
                "commits": get_commit_history(config, &remote_id)?,
            }))
        }),
        _ => Err(format!("Unknown command: {command}")),
    };

    match result {
        Ok(data) => CommandResponse::ok(data),
        Err(error) => CommandResponse::err(error),
    }
}
