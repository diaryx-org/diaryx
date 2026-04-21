//! File-backed [`AuthenticatedClient`] for the CLI.
//!
//! The session token lives in `~/.config/diaryx/auth.md` alongside the user
//! config, stored as a single field in the YAML frontmatter. The token is
//! private to this module — the only way out is [`FsAuthenticatedClient::export_bearer_token`],
//! which exists solely for handoff to guest plugin runtimes that need to make
//! their own authenticated HTTP calls.

use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use diaryx_core::auth::{
    AuthError, AuthMetadata, AuthenticatedClient, DEFAULT_SYNC_SERVER, HttpResponse,
};
use diaryx_core::config::Config;
use diaryx_core::frontmatter::{parse_typed, serialize_typed};
use diaryx_native::NativeConfigExt;

/// On-disk auth file format (private to this module).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AuthFile {
    #[serde(default = "default_auth_title")]
    title: String,
    #[serde(default = "default_auth_part_of")]
    part_of: String,
    server_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_id: Option<String>,
}

fn default_auth_title() -> String {
    "Auth".to_string()
}

fn default_auth_part_of() -> String {
    "config.md".to_string()
}

/// File-backed [`AuthenticatedClient`] for the CLI.
///
/// The server URL is fixed at construction time. To target a different server,
/// construct a new client. The token is loaded from disk on construction and
/// mirrored back to disk on every mutation.
pub struct FsAuthenticatedClient {
    server_url: String,
    auth_path: PathBuf,
    agent: ureq::Agent,
    state: Mutex<AuthFile>,
}

impl FsAuthenticatedClient {
    /// Construct a new client targeting `server_url`, with state persisted at
    /// `auth_path`. Loads existing state from disk if present; otherwise
    /// starts with an empty session.
    pub fn new(server_url: String, auth_path: PathBuf) -> Self {
        let server_url = server_url.trim_end_matches('/').to_string();

        let state = Self::read_file(&auth_path).unwrap_or_else(|| AuthFile {
            title: default_auth_title(),
            part_of: default_auth_part_of(),
            server_url: server_url.clone(),
            session_token: None,
            email: None,
            workspace_id: None,
        });

        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .build()
            .new_agent();

        Self {
            server_url,
            auth_path,
            agent,
            state: Mutex::new(state),
        }
    }

    /// Construct a client at the default auth path (`~/.config/diaryx/auth.md`).
    ///
    /// The server URL is resolved from `server_override`, then from stored
    /// state on disk, then from [`DEFAULT_SYNC_SERVER`].
    pub fn from_default_path(server_override: Option<&str>) -> Option<Self> {
        let auth_path = Config::config_path()?.with_file_name("auth.md");

        let stored = Self::read_file(&auth_path);
        let server_url = server_override
            .map(|s| s.trim_end_matches('/').to_string())
            .or_else(|| {
                stored
                    .as_ref()
                    .map(|s| s.server_url.trim_end_matches('/').to_string())
            })
            .unwrap_or_else(|| DEFAULT_SYNC_SERVER.to_string());

        Some(Self::new(server_url, auth_path))
    }

    /// Path to the auth file this client persists to.
    #[allow(dead_code)]
    pub fn auth_path(&self) -> &Path {
        &self.auth_path
    }

    /// **Escape hatch** — export the raw bearer token for handoff to guest
    /// plugin runtimes that need to make their own authenticated HTTP calls.
    /// Use only at well-defined trust boundaries (the token crosses a process
    /// or WASM sandbox boundary). Returns `None` when no session is active.
    #[allow(dead_code)] // only used when plugins feature is enabled
    pub fn export_bearer_token(&self) -> Option<String> {
        self.state.lock().ok()?.session_token.clone()
    }

    /// Read the on-disk state without constructing a full client. Used by
    /// callers that only need metadata (server URL, email, workspace ID).
    #[allow(dead_code)] // only used when plugins feature is enabled
    pub fn read_default_metadata() -> Option<(String, AuthMetadata)> {
        let auth_path = Config::config_path()?.with_file_name("auth.md");
        let state = Self::read_file(&auth_path)?;
        Some((
            state.server_url,
            AuthMetadata {
                email: state.email,
                workspace_id: state.workspace_id,
            },
        ))
    }

    fn read_file(path: &Path) -> Option<AuthFile> {
        if !path.exists() {
            return None;
        }
        let contents = std::fs::read_to_string(path).ok()?;
        parse_typed(&contents).ok()
    }

    fn write_file(&self, state: &AuthFile) -> std::io::Result<()> {
        if let Some(parent) = self.auth_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Ensure config.md (root index) exists so the mini-workspace is complete.
        self.ensure_config_md_exists();

        let contents = serialize_typed(state)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(&self.auth_path, contents)
    }

    fn ensure_config_md_exists(&self) {
        if let Some(parent) = self.auth_path.parent() {
            let config_path = parent.join("config.md");
            if !config_path.exists()
                && let Ok(config) = Config::load()
            {
                let _ = config.save();
            }
        }
    }

    fn build_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else if let Some(stripped) = path.strip_prefix('/') {
            format!("{}/{}", self.server_url, stripped)
        } else {
            format!("{}/{}", self.server_url, path)
        }
    }

    fn bearer_header(&self) -> Option<String> {
        let state = self.state.lock().ok()?;
        state.session_token.as_ref().map(|t| format!("Bearer {t}"))
    }

    fn finish(
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<HttpResponse, AuthError> {
        match result {
            Ok(mut resp) => {
                let status: u16 = resp.status().into();
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                Ok(HttpResponse { status, body })
            }
            Err(e) => Err(AuthError::network(e.to_string())),
        }
    }

    fn do_get(&self, path: &str, authed: bool) -> Result<HttpResponse, AuthError> {
        let url = self.build_url(path);
        let mut req = self.agent.get(&url);
        if authed && let Some(h) = self.bearer_header() {
            req = req.header("Authorization", &h);
        }
        Self::finish(req.call())
    }

    fn do_body(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
        authed: bool,
    ) -> Result<HttpResponse, AuthError> {
        let url = self.build_url(path);
        let mut req = match method {
            Method::Post => self.agent.post(&url),
            Method::Put => self.agent.put(&url),
            Method::Patch => self.agent.patch(&url),
        };
        req = req.header("Content-Type", "application/json");
        if authed && let Some(h) = self.bearer_header() {
            req = req.header("Authorization", &h);
        }
        Self::finish(req.send(body.unwrap_or("{}").as_bytes()))
    }

    fn do_delete(&self, path: &str, authed: bool) -> Result<HttpResponse, AuthError> {
        let url = self.build_url(path);
        let mut req = self.agent.delete(&url);
        if authed && let Some(h) = self.bearer_header() {
            req = req.header("Authorization", &h);
        }
        Self::finish(req.call())
    }

    fn mutate_state<F: FnOnce(&mut AuthFile)>(&self, f: F) -> AuthFile {
        let mut state = match self.state.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        f(&mut state);
        state.clone()
    }
}

enum Method {
    Post,
    Put,
    Patch,
}

#[async_trait::async_trait]
impl AuthenticatedClient for FsAuthenticatedClient {
    fn server_url(&self) -> &str {
        &self.server_url
    }

    async fn has_session(&self) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.session_token.clone())
            .is_some()
    }

    async fn load_metadata(&self) -> Option<AuthMetadata> {
        let state = self.state.lock().ok()?;
        Some(AuthMetadata {
            email: state.email.clone(),
            workspace_id: state.workspace_id.clone(),
        })
    }

    async fn save_metadata(&self, metadata: &AuthMetadata) {
        let snapshot = self.mutate_state(|state| {
            state.email = metadata.email.clone();
            state.workspace_id = metadata.workspace_id.clone();
        });
        let _ = self.write_file(&snapshot);
    }

    async fn store_session_token(&self, token: &str) {
        let snapshot = self.mutate_state(|state| {
            state.session_token = Some(token.to_string());
        });
        let _ = self.write_file(&snapshot);
    }

    async fn clear_session(&self) {
        let snapshot = self.mutate_state(|state| {
            state.session_token = None;
        });
        let _ = self.write_file(&snapshot);
    }

    async fn get(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.do_get(path, true)
    }

    async fn post(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.do_body(Method::Post, path, body, true)
    }

    async fn put(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.do_body(Method::Put, path, body, true)
    }

    async fn patch(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.do_body(Method::Patch, path, body, true)
    }

    async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.do_delete(path, true)
    }

    async fn get_unauth(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.do_get(path, false)
    }

    async fn post_unauth(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.do_body(Method::Post, path, body, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_auth_md() -> &'static str {
        concat!(
            "---\n",
            "title: Auth\n",
            "part_of: config.md\n",
            "server_url: https://sync.example.com\n",
            "session_token: secret-token\n",
            "email: user@example.com\n",
            "workspace_id: ws-1\n",
            "---\n",
        )
    }

    #[test]
    fn reads_existing_auth_file_on_construction() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.md");
        std::fs::write(&path, sample_auth_md()).unwrap();

        let client = FsAuthenticatedClient::new("https://sync.example.com".into(), path);
        assert_eq!(client.server_url(), "https://sync.example.com");
        assert_eq!(
            client.export_bearer_token().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn store_and_clear_token_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.md");
        let client = FsAuthenticatedClient::new("https://sync.example.com".into(), path.clone());

        futures_lite::future::block_on(client.store_session_token("new-tok"));
        assert_eq!(client.export_bearer_token().as_deref(), Some("new-tok"));

        // Re-open from disk, token should survive.
        drop(client);
        let reopened = FsAuthenticatedClient::new("https://sync.example.com".into(), path.clone());
        assert_eq!(reopened.export_bearer_token().as_deref(), Some("new-tok"));

        futures_lite::future::block_on(reopened.clear_session());
        assert_eq!(reopened.export_bearer_token(), None);
    }

    #[test]
    fn save_and_load_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.md");
        let client = FsAuthenticatedClient::new("https://sync.example.com".into(), path);

        futures_lite::future::block_on(client.save_metadata(&AuthMetadata {
            email: Some("x@y.com".into()),
            workspace_id: Some("ws-42".into()),
        }));

        let loaded = futures_lite::future::block_on(client.load_metadata()).unwrap();
        assert_eq!(loaded.email.as_deref(), Some("x@y.com"));
        assert_eq!(loaded.workspace_id.as_deref(), Some("ws-42"));
    }

    #[test]
    fn build_url_handles_leading_slash_and_trailing_slash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.md");
        let client = FsAuthenticatedClient::new("https://sync.example.com/".into(), path);

        assert_eq!(
            client.build_url("/auth/me"),
            "https://sync.example.com/auth/me"
        );
        assert_eq!(
            client.build_url("auth/me"),
            "https://sync.example.com/auth/me"
        );
    }
}
