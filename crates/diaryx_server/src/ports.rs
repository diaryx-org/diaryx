use crate::domain::{
    AudienceInfo, AuthSessionInfo, CustomDomainInfo, DeviceInfo, NamespaceInfo,
    NamespaceSessionInfo, ObjectMeta, UsageTotals, UserInfo, UserTier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServerCoreError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    RateLimited(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Internal(String),
}

impl ServerCoreError {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::RateLimited(message.into())
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}

impl From<String> for ServerCoreError {
    fn from(value: String) -> Self {
        Self::Internal(value)
    }
}

impl From<&str> for ServerCoreError {
    fn from(value: &str) -> Self {
        Self::Internal(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartCompletedPart {
    pub part_no: u32,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub subject: String,
    pub audience: Option<String>,
    pub expires_at_unix: Option<i64>,
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

crate::cfg_async_trait! {

pub trait AuthStore: Send + Sync {
    async fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, ServerCoreError>;
    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, ServerCoreError>;
    async fn rename_device(&self, device_id: &str, new_name: &str)
    -> Result<bool, ServerCoreError>;
    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError>;
    async fn get_user_tier(&self, user_id: &str) -> Result<UserTier, ServerCoreError>;
}

pub trait AuthSessionStore: Send + Sync {
    async fn validate_session(
        &self,
        token: &str,
    ) -> Result<Option<AuthSessionInfo>, ServerCoreError>;
    async fn create_auth_session(
        &self,
        user_id: &str,
        device_id: &str,
        expires_at_unix: i64,
    ) -> Result<String, ServerCoreError>;
    async fn delete_session(&self, token: &str) -> Result<(), ServerCoreError>;
    async fn update_device_last_seen(&self, device_id: &str) -> Result<(), ServerCoreError>;
}

pub trait MagicLinkStore: Send + Sync {
    /// Create a magic token + 6-digit code for the given email.
    /// Returns (token, code).
    async fn create_magic_token(
        &self,
        email: &str,
        expires_at_unix: i64,
    ) -> Result<(String, String), ServerCoreError>;
    /// Check whether a magic-link token is valid without consuming it.
    /// Returns the associated email on success.
    async fn peek_magic_token(
        &self,
        token: &str,
    ) -> Result<Option<String>, ServerCoreError>;
    /// Mark a magic-link token as consumed.
    async fn consume_magic_token(&self, token: &str) -> Result<(), ServerCoreError>;
    /// Check whether a magic code is valid without consuming it.
    /// Returns the associated email on success.
    async fn peek_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<Option<String>, ServerCoreError>;
    /// Mark a magic code as consumed.
    async fn consume_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<(), ServerCoreError>;
    /// Count magic tokens created for an email since the given unix timestamp.
    async fn count_recent_magic_tokens(
        &self,
        email: &str,
        since_unix: i64,
    ) -> Result<u64, ServerCoreError>;
}

pub trait UserStore: Send + Sync {
    /// Get or create a user by email. Returns the user ID.
    async fn get_or_create_user(
        &self,
        email: &str,
    ) -> Result<String, ServerCoreError>;
    /// Update the user's last login timestamp.
    async fn update_last_login(&self, user_id: &str) -> Result<(), ServerCoreError>;
    /// Delete a user and all associated data.
    async fn delete_user(&self, user_id: &str) -> Result<(), ServerCoreError>;
    /// Get the effective device limit for a user (per-user override or tier default).
    async fn get_effective_device_limit(&self, user_id: &str) -> Result<u32, ServerCoreError>;
    /// Set the user's billing tier.
    async fn set_user_tier(
        &self,
        user_id: &str,
        tier: UserTier,
    ) -> Result<(), ServerCoreError>;
}

pub trait DeviceStore: Send + Sync {
    /// Create a new device for a user. Returns the device ID.
    async fn create_device(
        &self,
        user_id: &str,
        name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String, ServerCoreError>;
    /// Count registered devices for a user.
    async fn count_user_devices(&self, user_id: &str) -> Result<u32, ServerCoreError>;
    /// List devices for a user.
    async fn list_user_devices(
        &self,
        user_id: &str,
    ) -> Result<Vec<DeviceInfo>, ServerCoreError>;
    /// Delete a device (and its sessions).
    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError>;
}

pub trait NamespaceStore: Send + Sync {
    async fn get_namespace(
        &self,
        namespace_id: &str,
    ) -> Result<Option<NamespaceInfo>, ServerCoreError>;
    async fn list_namespaces(
        &self,
        owner_user_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NamespaceInfo>, ServerCoreError>;
    async fn get_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<Option<AudienceInfo>, ServerCoreError>;
    async fn get_custom_domain(
        &self,
        domain: &str,
    ) -> Result<Option<CustomDomainInfo>, ServerCoreError>;
    async fn list_custom_domains(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<CustomDomainInfo>, ServerCoreError>;
    async fn upsert_custom_domain(
        &self,
        domain: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError>;
    async fn delete_custom_domain(&self, domain: &str) -> Result<bool, ServerCoreError>;

    async fn create_namespace(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        metadata: Option<&str>,
    ) -> Result<(), ServerCoreError>;
    async fn update_namespace_metadata(
        &self,
        namespace_id: &str,
        metadata: Option<&str>,
    ) -> Result<(), ServerCoreError>;
    async fn delete_namespace(&self, namespace_id: &str) -> Result<(), ServerCoreError>;

    async fn upsert_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
        access: &str,
    ) -> Result<(), ServerCoreError>;
    async fn list_audiences(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<AudienceInfo>, ServerCoreError>;
    async fn delete_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError>;
    async fn clear_objects_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError>;
}

pub trait SessionStore: Send + Sync {
    async fn create_session(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<i64>,
    ) -> Result<String, ServerCoreError>;
    async fn get_session(
        &self,
        code: &str,
    ) -> Result<Option<NamespaceSessionInfo>, ServerCoreError>;
    async fn update_session_read_only(
        &self,
        code: &str,
        read_only: bool,
    ) -> Result<bool, ServerCoreError>;
    async fn delete_session(&self, code: &str) -> Result<bool, ServerCoreError>;
}

pub trait ObjectMetaStore: Send + Sync {
    async fn upsert_object(
        &self,
        namespace_id: &str,
        key: &str,
        blob_key: &str,
        mime_type: &str,
        size_bytes: u64,
        audience: Option<&str>,
        content_hash: Option<&str>,
    ) -> Result<(), ServerCoreError>;
    async fn get_object_meta(
        &self,
        namespace_id: &str,
        key: &str,
    ) -> Result<Option<ObjectMeta>, ServerCoreError>;
    async fn list_objects(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ObjectMeta>, ServerCoreError>;
    async fn delete_object(&self, namespace_id: &str, key: &str) -> Result<(), ServerCoreError>;
    /// Count how many object metadata rows reference a given blob key within a namespace.
    async fn count_refs_to_blob(
        &self,
        namespace_id: &str,
        blob_key: &str,
    ) -> Result<u64, ServerCoreError>;
    async fn record_usage(
        &self,
        user_id: &str,
        event_type: &str,
        amount: u64,
        namespace_id: Option<&str>,
    ) -> Result<(), ServerCoreError>;
    async fn get_usage_totals(&self, user_id: &str) -> Result<UsageTotals, ServerCoreError>;
    async fn get_namespace_usage_totals(
        &self,
        user_id: &str,
        namespace_id: &str,
    ) -> Result<UsageTotals, ServerCoreError>;
}

pub trait BlobStore: Send + Sync {
    fn blob_key(&self, user_id: &str, hash: &str) -> String;
    fn prefix(&self) -> &str;

    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        metadata: Option<&HashMap<String, String>>,
    ) -> Result<(), ServerCoreError>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError>;
    async fn delete(&self, key: &str) -> Result<(), ServerCoreError>;
    async fn exists(&self, key: &str) -> Result<bool, ServerCoreError>;
    async fn init_multipart(&self, key: &str, mime_type: &str) -> Result<String, ServerCoreError>;
    async fn upload_part(
        &self,
        key: &str,
        multipart_id: &str,
        part_no: u32,
        bytes: &[u8],
    ) -> Result<String, ServerCoreError>;
    async fn complete_multipart(
        &self,
        key: &str,
        multipart_id: &str,
        parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError>;
    async fn abort_multipart(&self, key: &str, multipart_id: &str) -> Result<(), ServerCoreError>;
    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError>;
    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError>;
    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError>;
}

pub trait Mailer: Send + Sync {
    async fn send_magic_link(
        &self,
        to_email: &str,
        magic_link_url: &str,
        verification_code: &str,
    ) -> Result<(), ServerCoreError>;
}

pub trait RateLimitStore: Send + Sync {
    async fn check_and_increment(
        &self,
        scope: &str,
        key: &str,
        limit: u64,
        window_secs: u64,
    ) -> Result<bool, ServerCoreError>;
}

pub trait DomainMappingCache: Send + Sync {
    async fn put_domain(
        &self,
        hostname: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError>;
    async fn delete_domain(&self, hostname: &str) -> Result<(), ServerCoreError>;
    async fn put_subdomain(
        &self,
        subdomain: &str,
        namespace_id: &str,
        default_audience: Option<&str>,
    ) -> Result<(), ServerCoreError>;
    async fn delete_subdomain(&self, subdomain: &str) -> Result<(), ServerCoreError>;
}

pub trait BillingProvider: Send + Sync {
    async fn create_checkout_url(&self, user_id: &str) -> Result<String, ServerCoreError>;
    async fn create_portal_url(&self, user_id: &str) -> Result<String, ServerCoreError>;
}

pub trait AppleReceiptVerifier: Send + Sync {
    async fn verify_transaction(&self, signed_payload: &str) -> Result<Value, ServerCoreError>;
}

pub trait BillingStore: Send + Sync {
    // Stripe
    async fn get_stripe_customer_id(&self, user_id: &str) -> Result<Option<String>, ServerCoreError>;
    async fn set_stripe_customer_id(&self, user_id: &str, customer_id: &str) -> Result<(), ServerCoreError>;
    async fn get_user_id_by_stripe_customer_id(&self, customer_id: &str) -> Result<Option<String>, ServerCoreError>;
    async fn set_stripe_subscription_id(&self, user_id: &str, subscription_id: Option<&str>) -> Result<(), ServerCoreError>;
    // Apple
    async fn get_apple_original_transaction_id(&self, user_id: &str) -> Result<Option<String>, ServerCoreError>;
    async fn set_apple_original_transaction_id(&self, user_id: &str, transaction_id: &str) -> Result<(), ServerCoreError>;
    async fn get_user_id_by_apple_transaction_id(&self, transaction_id: &str) -> Result<Option<String>, ServerCoreError>;
}

pub trait AiProvider: Send + Sync {
    async fn chat_completion(&self, request: Value) -> Result<Value, ServerCoreError>;
}

pub trait JobSink: Send + Sync {
    async fn enqueue(&self, kind: &str, payload: Value) -> Result<(), ServerCoreError>;
}

} // cfg_async_trait!

pub trait TokenSigner: Send + Sync {
    fn sign(&self, claims: &TokenClaims) -> Result<String, ServerCoreError>;
    fn validate(&self, token: &str) -> Result<TokenClaims, ServerCoreError>;
}

pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}
