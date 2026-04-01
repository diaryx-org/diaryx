use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use crate::config::Config;

use super::{AuthCredentials, AuthStorage, DEFAULT_SYNC_SERVER};

/// Wrapper struct for serializing auth credentials with workspace hierarchy fields.
#[derive(serde::Serialize, serde::Deserialize)]
struct AuthFile {
    #[serde(default = "default_auth_title")]
    title: String,
    #[serde(default = "default_auth_part_of")]
    part_of: String,
    #[serde(flatten)]
    credentials: AuthCredentials,
}

fn default_auth_title() -> String {
    "Auth".to_string()
}

fn default_auth_part_of() -> String {
    "config.md".to_string()
}

/// Native file-backed auth storage for the Diaryx account session.
///
/// This store lives alongside the native user config and falls back to legacy
/// `Config.sync_*` fields when no dedicated auth file exists yet.
#[derive(Debug, Clone)]
pub struct NativeFileAuthStorage {
    path: PathBuf,
}

impl NativeFileAuthStorage {
    /// Create a file-backed auth store at an explicit path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create the default native auth store next to `config.md`.
    pub fn global() -> Option<Self> {
        Config::config_path().map(|path| Self::new(path.with_file_name("auth.md")))
    }

    /// Load stored credentials from the global auth store or legacy config.
    pub fn load_global_credentials() -> Option<AuthCredentials> {
        Self::global().and_then(|storage| storage.load_credentials_blocking())
    }

    /// Persist credentials to the global auth store.
    pub fn save_global_credentials(credentials: &AuthCredentials) -> Result<()> {
        let storage = Self::global().ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "No config directory available for auth storage",
            )
        })?;
        storage.save_credentials_blocking(credentials)
    }

    /// Clear only the stored session token while keeping server/email metadata.
    pub fn clear_global_session() -> Result<()> {
        if let Some(storage) = Self::global() {
            storage.clear_session_blocking()?;
        }
        clear_legacy_session()
    }

    fn load_credentials_blocking(&self) -> Option<AuthCredentials> {
        read_credentials_file(&self.path)
            .or_else(|| read_legacy_toml_credentials(&self.path))
            .or_else(load_legacy_credentials)
    }

    fn save_credentials_blocking(&self, credentials: &AuthCredentials) -> Result<()> {
        write_credentials_file(&self.path, credentials)
    }

    fn clear_session_blocking(&self) -> Result<()> {
        let mut credentials = self
            .load_credentials_blocking()
            .unwrap_or_else(default_credentials);
        credentials.session_token = None;
        self.save_credentials_blocking(&credentials)
    }
}

fn default_credentials() -> AuthCredentials {
    AuthCredentials {
        server_url: DEFAULT_SYNC_SERVER.to_string(),
        session_token: None,
        email: None,
        workspace_id: None,
    }
}

fn read_credentials_file(path: &Path) -> Option<AuthCredentials> {
    if !path.exists() {
        return None;
    }

    let contents = fs::read_to_string(path).ok()?;

    // If it starts with frontmatter delimiters, parse as YAML frontmatter
    if contents.starts_with("---\n") || contents.starts_with("---\r\n") {
        let auth_file: AuthFile = crate::frontmatter::parse_typed(&contents).ok()?;
        return Some(auth_file.credentials);
    }

    // Otherwise try TOML (legacy auth.toml that was renamed to auth.md)
    toml::from_str(&contents).ok()
}

/// Try to read the legacy auth.toml file and migrate to auth.md.
fn read_legacy_toml_credentials(md_path: &Path) -> Option<AuthCredentials> {
    let toml_path = md_path.with_extension("toml");
    if !toml_path.exists() {
        return None;
    }

    let contents = fs::read_to_string(&toml_path).ok()?;
    let credentials: AuthCredentials = toml::from_str(&contents).ok()?;

    // Migrate: write as markdown and remove old file
    let _ = write_credentials_file(md_path, &credentials);
    let _ = fs::remove_file(&toml_path);

    Some(credentials)
}

fn write_credentials_file(path: &Path, credentials: &AuthCredentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Ensure config.md (root index) exists so the mini-workspace is complete
    ensure_config_md_exists(path);

    let auth_file = AuthFile {
        title: default_auth_title(),
        part_of: default_auth_part_of(),
        credentials: credentials.clone(),
    };

    let contents = crate::frontmatter::serialize_typed(&auth_file)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    fs::write(path, contents)
}

/// Ensure config.md exists in the same directory so auth.md has a valid parent index.
fn ensure_config_md_exists(auth_path: &Path) {
    if let Some(parent) = auth_path.parent() {
        let config_path = parent.join("config.md");
        if !config_path.exists() {
            // Load (or create default) config and save it as config.md
            if let Ok(config) = Config::load() {
                let _ = config.save();
            }
        }
    }
}

fn load_legacy_credentials() -> Option<AuthCredentials> {
    let config = Config::load().ok()?;
    let has_legacy_auth = config.sync_server_url.is_some()
        || config.sync_session_token.is_some()
        || config.sync_email.is_some()
        || config.sync_workspace_id.is_some();
    if !has_legacy_auth {
        return None;
    }

    Some(AuthCredentials {
        server_url: config
            .sync_server_url
            .unwrap_or_else(|| DEFAULT_SYNC_SERVER.to_string()),
        session_token: config.sync_session_token,
        email: config.sync_email,
        workspace_id: config.sync_workspace_id,
    })
}

fn clear_legacy_session() -> Result<()> {
    let mut config = Config::load().map_err(|e| Error::other(e.to_string()))?;
    if config.sync_session_token.take().is_none() {
        return Ok(());
    }
    config.save().map_err(|e| Error::other(e.to_string()))
}

#[async_trait::async_trait]
impl AuthStorage for NativeFileAuthStorage {
    async fn load_credentials(&self) -> Option<AuthCredentials> {
        self.load_credentials_blocking()
    }

    async fn save_credentials(&self, credentials: &AuthCredentials) {
        let _ = self.save_credentials_blocking(credentials);
    }

    async fn clear_session(&self) {
        let _ = self.clear_session_blocking();
        let _ = clear_legacy_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_storage_round_trips_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let storage = NativeFileAuthStorage::new(temp.path().join("auth.md"));
        let credentials = AuthCredentials {
            server_url: "https://sync.example.com".into(),
            session_token: Some("token-123".into()),
            email: Some("user@example.com".into()),
            workspace_id: Some("remote-1".into()),
        };

        storage.save_credentials_blocking(&credentials).unwrap();

        // Verify file is markdown with frontmatter
        let contents = fs::read_to_string(temp.path().join("auth.md")).unwrap();
        assert!(contents.starts_with("---\n"));
        assert!(contents.contains("part_of: config.md"));
        assert!(contents.contains("title: Auth"));

        assert_eq!(storage.load_credentials_blocking(), Some(credentials));
    }

    #[test]
    fn clear_session_preserves_non_secret_fields() {
        let temp = tempfile::tempdir().unwrap();
        let storage = NativeFileAuthStorage::new(temp.path().join("auth.md"));
        storage
            .save_credentials_blocking(&AuthCredentials {
                server_url: "https://sync.example.com".into(),
                session_token: Some("token-123".into()),
                email: Some("user@example.com".into()),
                workspace_id: Some("remote-1".into()),
            })
            .unwrap();

        storage.clear_session_blocking().unwrap();

        assert_eq!(
            storage.load_credentials_blocking(),
            Some(AuthCredentials {
                server_url: "https://sync.example.com".into(),
                session_token: None,
                email: Some("user@example.com".into()),
                workspace_id: Some("remote-1".into()),
            })
        );
    }

    #[test]
    fn migrates_from_toml_to_md() {
        let temp = tempfile::tempdir().unwrap();
        let toml_path = temp.path().join("auth.toml");
        let md_path = temp.path().join("auth.md");

        // Write legacy TOML file
        let credentials = AuthCredentials {
            server_url: "https://sync.example.com".into(),
            session_token: Some("token-legacy".into()),
            email: Some("legacy@example.com".into()),
            workspace_id: None,
        };
        let toml_str = toml::to_string_pretty(&credentials).unwrap();
        fs::write(&toml_path, toml_str).unwrap();

        let storage = NativeFileAuthStorage::new(md_path.clone());
        let loaded = storage.load_credentials_blocking().unwrap();
        assert_eq!(loaded.session_token.as_deref(), Some("token-legacy"));

        // TOML file should be gone, MD file should exist
        assert!(!toml_path.exists());
        assert!(md_path.exists());

        // MD file should have frontmatter
        let contents = fs::read_to_string(&md_path).unwrap();
        assert!(contents.starts_with("---\n"));
    }
}
