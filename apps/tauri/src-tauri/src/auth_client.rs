//! Keyring-backed [`AuthenticatedClient`] for the Tauri host.
//!
//! The session token lives in the OS keyring (service `"org.diaryx.app"`,
//! user `"session_token"`). Non-secret metadata — server URL, last known
//! email, workspace ID — lives in a small JSON file under the app's local
//! data directory (`<app_data>/auth.json`) so that the web layer can read
//! it without unlocking the keyring.
//!
//! HTTP requests are sent from this Rust process via `reqwest`; the raw
//! session token never crosses the IPC boundary into JavaScript.

use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use diaryx_core::auth::{
    AuthError, AuthMetadata, AuthenticatedClient, DEFAULT_SYNC_SERVER, HttpResponse,
};
use serde::{Deserialize, Serialize};

/// OS keyring service name — shared with the legacy credential bridge so
/// existing tokens stored by the web-side flow remain readable after the
/// migration. (Individual slots are disambiguated by the keyring *account*
/// field, not the service name.)
const KEYRING_SERVICE: &str = "org.diaryx.app";
/// Keyring account for the session bearer token.
const KEYRING_TOKEN_ACCOUNT: &str = "session_token";

/// Persistent metadata file name inside the Tauri app-data directory.
const METADATA_FILENAME: &str = "auth.json";

/// On-disk metadata format (private to this module).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MetadataFile {
    server_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_id: Option<String>,
}

/// Keyring-backed [`AuthenticatedClient`] for Tauri.
pub struct KeyringAuthenticatedClient {
    server_url: String,
    metadata_path: PathBuf,
    http: reqwest::Client,
    state: Mutex<MetadataFile>,
}

impl KeyringAuthenticatedClient {
    /// Construct a client targeting `server_url`, persisting metadata at
    /// `metadata_path`. Loads existing metadata (and the stored session
    /// token, implicitly via the keyring) if present.
    pub fn new(server_url: String, metadata_path: PathBuf) -> Self {
        let server_url = server_url.trim_end_matches('/').to_string();

        let mut state = Self::read_file(&metadata_path).unwrap_or_default();
        // Always sync the server_url field to what the caller asked for.
        state.server_url = server_url.clone();

        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            server_url,
            metadata_path,
            http,
            state: Mutex::new(state),
        }
    }

    /// Construct a client at the default metadata path inside `app_data_dir`.
    ///
    /// The server URL is resolved from `server_override`, then from stored
    /// metadata on disk, then from [`DEFAULT_SYNC_SERVER`].
    pub fn from_app_data_dir(app_data_dir: &Path, server_override: Option<&str>) -> Self {
        let metadata_path = app_data_dir.join(METADATA_FILENAME);
        let stored = Self::read_file(&metadata_path);
        let server_url = server_override
            .map(|s| s.trim_end_matches('/').to_string())
            .or_else(|| {
                stored.as_ref().and_then(|s| {
                    let trimmed = s.server_url.trim_end_matches('/').to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                })
            })
            .unwrap_or_else(|| DEFAULT_SYNC_SERVER.to_string());

        Self::new(server_url, metadata_path)
    }

    /// Path to the metadata file this client persists to.
    #[allow(dead_code)]
    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    fn read_file(path: &Path) -> Option<MetadataFile> {
        if !path.exists() {
            return None;
        }
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn write_file(&self, state: &MetadataFile) -> std::io::Result<()> {
        if let Some(parent) = self.metadata_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(state)
            .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(&self.metadata_path, contents)
    }

    fn mutate_state<F: FnOnce(&mut MetadataFile)>(&self, f: F) -> MetadataFile {
        let mut state = match self.state.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        f(&mut state);
        state.clone()
    }

    // ------------------------------------------------------------------------
    // Keyring access — isolated so the raw token only flows through these.
    // ------------------------------------------------------------------------

    fn keyring_entry() -> Option<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_TOKEN_ACCOUNT).ok()
    }

    fn load_token() -> Option<String> {
        Self::keyring_entry()?.get_password().ok()
    }

    fn store_token(token: &str) -> bool {
        match Self::keyring_entry() {
            Some(entry) => entry.set_password(token).is_ok(),
            None => false,
        }
    }

    fn delete_token() -> bool {
        match Self::keyring_entry() {
            Some(entry) => entry.delete_credential().is_ok(),
            None => false,
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

    /// Internal transport helper. Takes owned data so that async-trait
    /// impls can call it with a simple `.await` without any borrows crossing
    /// the await boundary (which would break async-trait's generated
    /// lifetime bounds).
    async fn send(
        &self,
        method: reqwest::Method,
        path: String,
        body: Option<String>,
        authed: bool,
    ) -> Result<HttpResponse, AuthError> {
        let url = self.build_url(&path);
        let mut req = self.http.request(method, &url);

        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").body(b);
        }

        if authed && let Some(token) = Self::load_token() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }

        let resp = req
            .send()
            .await
            .map_err(|e| AuthError::network(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| AuthError::network(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

#[async_trait::async_trait]
impl AuthenticatedClient for KeyringAuthenticatedClient {
    fn server_url(&self) -> &str {
        &self.server_url
    }

    async fn has_session(&self) -> bool {
        Self::load_token().is_some()
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
        Self::store_token(token);
    }

    async fn clear_session(&self) {
        Self::delete_token();
    }

    async fn get(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.send(reqwest::Method::GET, path.to_string(), None, true)
            .await
    }

    async fn post(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.send(
            reqwest::Method::POST,
            path.to_string(),
            body.map(ToString::to_string),
            true,
        )
        .await
    }

    async fn put(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.send(
            reqwest::Method::PUT,
            path.to_string(),
            body.map(ToString::to_string),
            true,
        )
        .await
    }

    async fn patch(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.send(
            reqwest::Method::PATCH,
            path.to_string(),
            body.map(ToString::to_string),
            true,
        )
        .await
    }

    async fn delete(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.send(reqwest::Method::DELETE, path.to_string(), None, true)
            .await
    }

    async fn get_unauth(&self, path: &str) -> Result<HttpResponse, AuthError> {
        self.send(reqwest::Method::GET, path.to_string(), None, false)
            .await
    }

    async fn post_unauth(&self, path: &str, body: Option<&str>) -> Result<HttpResponse, AuthError> {
        self.send(
            reqwest::Method::POST,
            path.to_string(),
            body.map(ToString::to_string),
            false,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reads_existing_metadata_on_construction() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(METADATA_FILENAME);
        std::fs::write(
            &path,
            r#"{"server_url":"https://sync.example.com","email":"u@e.com","workspace_id":"ws-1"}"#,
        )
        .unwrap();

        let client = KeyringAuthenticatedClient::new("https://sync.example.com".into(), path);
        assert_eq!(client.server_url(), "https://sync.example.com");

        let meta = futures_lite::future::block_on(client.load_metadata()).unwrap();
        assert_eq!(meta.email.as_deref(), Some("u@e.com"));
        assert_eq!(meta.workspace_id.as_deref(), Some("ws-1"));
    }

    #[test]
    fn save_and_load_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(METADATA_FILENAME);
        let client = KeyringAuthenticatedClient::new("https://sync.example.com".into(), path);

        futures_lite::future::block_on(client.save_metadata(&AuthMetadata {
            email: Some("x@y.com".into()),
            workspace_id: Some("ws-42".into()),
        }));

        let loaded = futures_lite::future::block_on(client.load_metadata()).unwrap();
        assert_eq!(loaded.email.as_deref(), Some("x@y.com"));
        assert_eq!(loaded.workspace_id.as_deref(), Some("ws-42"));
    }

    #[test]
    fn build_url_handles_leading_and_trailing_slash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(METADATA_FILENAME);
        let client = KeyringAuthenticatedClient::new("https://sync.example.com/".into(), path);

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
