use crate::domain::{
    AudienceInfo, CustomDomainInfo, DeviceInfo, NamespaceInfo, UserInfo, UserTier,
};
use async_trait::async_trait;
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

#[async_trait]
pub trait AuthStore: Send + Sync {
    async fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, ServerCoreError>;
    async fn list_user_devices(&self, user_id: &str) -> Result<Vec<DeviceInfo>, ServerCoreError>;
    async fn rename_device(&self, device_id: &str, new_name: &str)
    -> Result<bool, ServerCoreError>;
    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError>;
    async fn get_user_tier(&self, user_id: &str) -> Result<UserTier, ServerCoreError>;
}

#[async_trait]
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
}

#[async_trait]
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

pub trait TokenSigner: Send + Sync {
    fn sign(&self, claims: &TokenClaims) -> Result<String, ServerCoreError>;
    fn validate(&self, token: &str) -> Result<TokenClaims, ServerCoreError>;
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send_magic_link(
        &self,
        to_email: &str,
        magic_link_url: &str,
        verification_code: &str,
    ) -> Result<(), ServerCoreError>;
}

#[async_trait]
pub trait RateLimitStore: Send + Sync {
    async fn check_and_increment(
        &self,
        scope: &str,
        key: &str,
        limit: u64,
        window_secs: u64,
    ) -> Result<bool, ServerCoreError>;
}

#[async_trait]
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

#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn create_checkout_url(&self, user_id: &str) -> Result<String, ServerCoreError>;
    async fn create_portal_url(&self, user_id: &str) -> Result<String, ServerCoreError>;
}

#[async_trait]
pub trait AppleReceiptVerifier: Send + Sync {
    async fn verify_transaction(&self, signed_payload: &str) -> Result<Value, ServerCoreError>;
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat_completion(&self, request: Value) -> Result<Value, ServerCoreError>;
}

pub trait Clock: Send + Sync {
    fn now_unix(&self) -> i64;
}

#[async_trait]
pub trait JobSink: Send + Sync {
    async fn enqueue(&self, kind: &str, payload: Value) -> Result<(), ServerCoreError>;
}
