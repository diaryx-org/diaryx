use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tier-based defaults for Diaryx cloud accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierDefaults {
    pub device_limit: u32,
    pub attachment_limit_bytes: u64,
    pub workspace_limit: u32,
    pub published_site_limit: u32,
}

/// User account tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserTier {
    Free,
    Plus,
}

impl UserTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserTier::Free => "free",
            UserTier::Plus => "plus",
        }
    }

    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "plus" => UserTier::Plus,
            _ => UserTier::Free,
        }
    }

    pub fn defaults(&self) -> TierDefaults {
        match self {
            UserTier::Free => TierDefaults {
                device_limit: 2,
                attachment_limit_bytes: 200 * 1024 * 1024,
                workspace_limit: 1,
                published_site_limit: 1,
            },
            UserTier::Plus => TierDefaults {
                device_limit: 10,
                attachment_limit_bytes: 2 * 1024 * 1024 * 1024,
                workspace_limit: 10,
                published_site_limit: 1,
            },
        }
    }
}

/// User account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub attachment_limit_bytes: Option<u64>,
    pub workspace_limit: Option<u32>,
    pub tier: UserTier,
    pub published_site_limit: Option<u32>,
}

/// Registered user device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub user_id: String,
    pub name: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// Authenticated session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSessionInfo {
    pub token: String,
    pub user_id: String,
    pub device_id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Result of validating an auth session — the user and session together.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub session: AuthSessionInfo,
    pub user: UserInfo,
}

/// Namespace metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceInfo {
    pub id: String,
    pub owner_user_id: String,
    pub created_at: i64,
}

/// Namespace audience metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudienceInfo {
    pub namespace_id: String,
    pub audience_name: String,
    pub access: String,
}

/// Namespace-scoped share session metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceSessionInfo {
    pub code: String,
    pub namespace_id: String,
    pub owner_user_id: String,
    pub read_only: bool,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

/// Custom domain mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDomainInfo {
    pub domain: String,
    pub namespace_id: String,
    pub audience_name: String,
    pub created_at: i64,
    pub verified: bool,
}

/// Metadata for an object stored in a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub namespace_id: String,
    pub key: String,
    /// Blob store key (e.g. R2 object key), or `None` for inline objects.
    pub blob_key: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub updated_at: i64,
    /// Audience tag. `None` = private (owner-only).
    pub audience: Option<String>,
    /// SHA-256 hex digest of the blob content. `None` for legacy rows.
    pub content_hash: Option<String>,
}

/// Aggregated usage totals for a user.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageTotals {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub relay_seconds: u64,
}

/// Resolved public access info for an object.
#[derive(Debug, Clone)]
pub struct PublicObjectAccess {
    pub meta: ObjectMeta,
    pub access: String,
    pub audience_name: String,
}

/// Portable result of "load current user context".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentUserContext {
    pub user: UserInfo,
    pub devices: Vec<DeviceInfo>,
    pub namespaces: Vec<NamespaceInfo>,
    pub limits: TierDefaults,
}
