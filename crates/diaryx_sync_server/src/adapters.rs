use crate::db::{AuthRepo, NamespaceRepo};
use async_trait::async_trait;
use diaryx_server::domain::{
    AudienceInfo as CoreAudienceInfo, AuthSessionInfo as CoreAuthSessionInfo,
    CustomDomainInfo as CoreCustomDomainInfo, DeviceInfo as CoreDeviceInfo,
    NamespaceInfo as CoreNamespaceInfo, NamespaceSessionInfo as CoreNamespaceSessionInfo,
    ObjectMeta as CoreObjectMeta, UsageTotals as CoreUsageTotals, UserInfo as CoreUserInfo,
    UserTier as CoreUserTier,
};
use diaryx_server::ports::{
    AuthSessionStore, AuthStore, DeviceStore, DomainMappingCache, MagicLinkStore, NamespaceStore,
    ObjectMetaStore, ServerCoreError, SessionStore, UserStore,
};
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

#[derive(Clone)]
pub struct NativeAuthStore {
    repo: Arc<AuthRepo>,
}

impl NativeAuthStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl AuthStore for NativeAuthStore {
    async fn get_user(&self, user_id: &str) -> Result<Option<CoreUserInfo>, ServerCoreError> {
        self.repo
            .get_user(user_id)
            .map(|user| user.map(Into::into))
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn list_user_devices(
        &self,
        user_id: &str,
    ) -> Result<Vec<CoreDeviceInfo>, ServerCoreError> {
        self.repo
            .get_user_devices(user_id)
            .map(|devices| devices.into_iter().map(Into::into).collect())
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn rename_device(
        &self,
        device_id: &str,
        new_name: &str,
    ) -> Result<bool, ServerCoreError> {
        self.repo
            .rename_device(device_id, new_name)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_device(device_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn get_user_tier(&self, user_id: &str) -> Result<CoreUserTier, ServerCoreError> {
        self.repo
            .get_user_tier(user_id)
            .map(Into::into)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }
}

#[derive(Clone)]
pub struct NativeAuthSessionStore {
    repo: Arc<AuthRepo>,
}

impl NativeAuthSessionStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl AuthSessionStore for NativeAuthSessionStore {
    async fn validate_session(
        &self,
        token: &str,
    ) -> Result<Option<CoreAuthSessionInfo>, ServerCoreError> {
        self.repo
            .validate_session(token)
            .map(|opt| opt.map(Into::into))
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn create_auth_session(
        &self,
        user_id: &str,
        device_id: &str,
        expires_at_unix: i64,
    ) -> Result<String, ServerCoreError> {
        let expires_at =
            chrono::DateTime::from_timestamp(expires_at_unix, 0).unwrap_or_else(chrono::Utc::now);
        self.repo
            .create_session(user_id, device_id, expires_at)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn delete_session(&self, token: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_session(token)
            .map(|_| ())
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn update_device_last_seen(&self, device_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .update_device_last_seen(device_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }
}

#[derive(Clone)]
pub struct NativeMagicLinkStore {
    repo: Arc<AuthRepo>,
}

impl NativeMagicLinkStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl MagicLinkStore for NativeMagicLinkStore {
    async fn create_magic_token(
        &self,
        email: &str,
        expires_at_unix: i64,
    ) -> Result<(String, String), ServerCoreError> {
        let expires_at =
            chrono::DateTime::from_timestamp(expires_at_unix, 0).unwrap_or_else(chrono::Utc::now);
        self.repo
            .create_magic_token(email, expires_at)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn peek_magic_token(&self, token: &str) -> Result<Option<String>, ServerCoreError> {
        self.repo
            .peek_magic_token(token)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn consume_magic_token(&self, token: &str) -> Result<(), ServerCoreError> {
        self.repo
            .consume_magic_token(token)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn peek_magic_code(
        &self,
        code: &str,
        email: &str,
    ) -> Result<Option<String>, ServerCoreError> {
        self.repo
            .peek_magic_code(code, email)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn consume_magic_code(&self, code: &str, email: &str) -> Result<(), ServerCoreError> {
        self.repo
            .consume_magic_code(code, email)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn count_recent_magic_tokens(
        &self,
        email: &str,
        since_unix: i64,
    ) -> Result<u64, ServerCoreError> {
        let since =
            chrono::DateTime::from_timestamp(since_unix, 0).unwrap_or_else(chrono::Utc::now);
        self.repo
            .count_recent_magic_tokens(email, since)
            .map(|c| c.max(0) as u64)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }
}

#[derive(Clone)]
pub struct NativeUserStore {
    repo: Arc<AuthRepo>,
}

impl NativeUserStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl UserStore for NativeUserStore {
    async fn get_or_create_user(&self, email: &str) -> Result<String, ServerCoreError> {
        self.repo
            .get_or_create_user(email)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn update_last_login(&self, user_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .update_last_login(user_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn delete_user(&self, user_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_user(user_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn get_effective_device_limit(&self, user_id: &str) -> Result<u32, ServerCoreError> {
        self.repo
            .get_effective_device_limit(user_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn set_user_tier(
        &self,
        user_id: &str,
        tier: diaryx_server::UserTier,
    ) -> Result<(), ServerCoreError> {
        let db_tier = match tier {
            diaryx_server::UserTier::Free => crate::db::UserTier::Free,
            diaryx_server::UserTier::Plus => crate::db::UserTier::Plus,
        };
        self.repo
            .set_user_tier(user_id, db_tier)
            .map(|_| ())
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }
}

#[derive(Clone)]
pub struct NativeDeviceStore {
    repo: Arc<AuthRepo>,
}

impl NativeDeviceStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl DeviceStore for NativeDeviceStore {
    async fn create_device(
        &self,
        user_id: &str,
        name: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<String, ServerCoreError> {
        self.repo
            .create_device(user_id, name, user_agent)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn count_user_devices(&self, user_id: &str) -> Result<u32, ServerCoreError> {
        self.repo
            .count_user_devices(user_id)
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn list_user_devices(
        &self,
        user_id: &str,
    ) -> Result<Vec<diaryx_server::DeviceInfo>, ServerCoreError> {
        self.repo
            .get_user_devices(user_id)
            .map(|devices| devices.into_iter().map(Into::into).collect())
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }

    async fn delete_device(&self, device_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_device(device_id)
            .map(|_| ())
            .map_err(|e| ServerCoreError::internal(e.to_string()))
    }
}

impl From<crate::db::SessionInfo> for CoreAuthSessionInfo {
    fn from(value: crate::db::SessionInfo) -> Self {
        Self {
            token: value.token,
            user_id: value.user_id,
            device_id: value.device_id,
            expires_at: value.expires_at,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone)]
pub struct NativeNamespaceStore {
    repo: Arc<NamespaceRepo>,
}

impl NativeNamespaceStore {
    pub fn new(repo: Arc<NamespaceRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl NamespaceStore for NativeNamespaceStore {
    async fn get_namespace(
        &self,
        namespace_id: &str,
    ) -> Result<Option<CoreNamespaceInfo>, ServerCoreError> {
        Ok(self.repo.get_namespace(namespace_id).map(Into::into))
    }

    async fn list_namespaces(
        &self,
        owner_user_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<CoreNamespaceInfo>, ServerCoreError> {
        Ok(self
            .repo
            .list_namespaces(owner_user_id, limit, offset)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn get_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<Option<CoreAudienceInfo>, ServerCoreError> {
        Ok(self
            .repo
            .get_audience(namespace_id, audience_name)
            .map(Into::into))
    }

    async fn get_custom_domain(
        &self,
        domain: &str,
    ) -> Result<Option<CoreCustomDomainInfo>, ServerCoreError> {
        Ok(self.repo.get_custom_domain(domain).map(Into::into))
    }

    async fn list_custom_domains(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<CoreCustomDomainInfo>, ServerCoreError> {
        Ok(self
            .repo
            .list_custom_domains(namespace_id)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn upsert_custom_domain(
        &self,
        domain: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .upsert_custom_domain(domain, namespace_id, audience_name)
            .map_err(ServerCoreError::from)
    }

    async fn delete_custom_domain(&self, domain: &str) -> Result<bool, ServerCoreError> {
        self.repo
            .delete_custom_domain(domain)
            .map_err(ServerCoreError::from)
    }

    async fn create_namespace(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .create_namespace(namespace_id, owner_user_id)
            .map_err(ServerCoreError::from)
    }

    async fn delete_namespace(&self, namespace_id: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_namespace(namespace_id)
            .map_err(ServerCoreError::from)
    }

    async fn upsert_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
        access: &str,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .upsert_audience(namespace_id, audience_name, access)
            .map_err(ServerCoreError::from)
    }

    async fn list_audiences(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<CoreAudienceInfo>, ServerCoreError> {
        Ok(self
            .repo
            .list_audiences(namespace_id)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn delete_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .delete_audience(namespace_id, audience_name)
            .map_err(ServerCoreError::from)
    }

    async fn clear_objects_audience(
        &self,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .clear_objects_audience(namespace_id, audience_name)
            .map(|_| ())
            .map_err(ServerCoreError::from)
    }
}

#[derive(Clone)]
pub struct NativeSessionStore {
    repo: Arc<NamespaceRepo>,
}

impl NativeSessionStore {
    pub fn new(repo: Arc<NamespaceRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl SessionStore for NativeSessionStore {
    async fn create_session(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        read_only: bool,
        expires_at: Option<i64>,
    ) -> Result<String, ServerCoreError> {
        self.repo
            .create_session(namespace_id, owner_user_id, read_only, expires_at)
            .map_err(ServerCoreError::from)
    }

    async fn get_session(
        &self,
        code: &str,
    ) -> Result<Option<CoreNamespaceSessionInfo>, ServerCoreError> {
        Ok(self.repo.get_session(code).map(Into::into))
    }

    async fn update_session_read_only(
        &self,
        code: &str,
        read_only: bool,
    ) -> Result<bool, ServerCoreError> {
        self.repo
            .update_session_read_only(code, read_only)
            .map_err(ServerCoreError::from)
    }

    async fn delete_session(&self, code: &str) -> Result<bool, ServerCoreError> {
        self.repo
            .delete_session(code)
            .map_err(ServerCoreError::from)
    }
}

#[derive(Clone)]
pub struct NativeDomainMappingCache {
    http_client: reqwest::Client,
    cf_account_id: String,
    kv_api_token: Option<String>,
    kv_namespace_id: Option<String>,
}

impl NativeDomainMappingCache {
    pub fn new(
        http_client: reqwest::Client,
        cf_account_id: impl Into<String>,
        kv_api_token: Option<String>,
        kv_namespace_id: Option<String>,
    ) -> Self {
        Self {
            http_client,
            cf_account_id: cf_account_id.into(),
            kv_api_token,
            kv_namespace_id,
        }
    }

    fn kv_value_url(&self, key: &str) -> Option<String> {
        let token = self.kv_api_token.as_ref()?;
        let namespace_id = self.kv_namespace_id.as_ref()?;
        if self.cf_account_id.is_empty() || token.is_empty() || namespace_id.is_empty() {
            return None;
        }

        Some(format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces/{}/values/{}",
            self.cf_account_id, namespace_id, key
        ))
    }

    async fn put_value(&self, key: &str, body: serde_json::Value, context: &str) {
        let Some(url) = self.kv_value_url(key) else {
            return;
        };
        let Some(token) = &self.kv_api_token else {
            return;
        };

        match self
            .http_client
            .put(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(response) if !response.status().is_success() => {
                warn!(
                    "Failed to sync {} to Cloudflare KV: status={}",
                    context,
                    response.status()
                );
            }
            Ok(_) => {}
            Err(err) => {
                warn!("Failed to sync {} to Cloudflare KV: {}", context, err);
            }
        }
    }

    async fn delete_value(&self, key: &str, context: &str) {
        let Some(url) = self.kv_value_url(key) else {
            return;
        };
        let Some(token) = &self.kv_api_token else {
            return;
        };

        match self
            .http_client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
        {
            Ok(response) if !response.status().is_success() => {
                warn!(
                    "Failed to delete {} from Cloudflare KV: status={}",
                    context,
                    response.status()
                );
            }
            Ok(_) => {}
            Err(err) => {
                warn!("Failed to delete {} from Cloudflare KV: {}", context, err);
            }
        }
    }
}

#[async_trait]
impl DomainMappingCache for NativeDomainMappingCache {
    async fn put_domain(
        &self,
        hostname: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        self.put_value(
            &format!("domain:{}", hostname),
            json!({
                "namespace_id": namespace_id,
                "audience_name": audience_name,
            }),
            &format!("domain '{}'", hostname),
        )
        .await;
        Ok(())
    }

    async fn delete_domain(&self, hostname: &str) -> Result<(), ServerCoreError> {
        self.delete_value(
            &format!("domain:{}", hostname),
            &format!("domain '{}'", hostname),
        )
        .await;
        Ok(())
    }

    async fn put_subdomain(
        &self,
        subdomain: &str,
        namespace_id: &str,
        default_audience: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let mut body = json!({
            "namespace_id": namespace_id,
        });
        if let Some(default_audience) = default_audience {
            body["default_audience"] = serde_json::Value::String(default_audience.to_string());
        }

        self.put_value(
            &format!("subdomain:{}", subdomain.to_lowercase()),
            body,
            &format!("subdomain '{}'", subdomain),
        )
        .await;
        Ok(())
    }

    async fn delete_subdomain(&self, subdomain: &str) -> Result<(), ServerCoreError> {
        self.delete_value(
            &format!("subdomain:{}", subdomain.to_lowercase()),
            &format!("subdomain '{}'", subdomain),
        )
        .await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct NativeObjectMetaStore {
    repo: Arc<NamespaceRepo>,
}

impl NativeObjectMetaStore {
    pub fn new(repo: Arc<NamespaceRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ObjectMetaStore for NativeObjectMetaStore {
    async fn upsert_object(
        &self,
        namespace_id: &str,
        key: &str,
        blob_key: &str,
        mime_type: &str,
        size_bytes: u64,
        audience: Option<&str>,
        content_hash: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .upsert_object(
                namespace_id,
                key,
                blob_key,
                mime_type,
                size_bytes,
                audience,
                content_hash,
            )
            .map_err(ServerCoreError::from)
    }

    async fn get_object_meta(
        &self,
        namespace_id: &str,
        key: &str,
    ) -> Result<Option<CoreObjectMeta>, ServerCoreError> {
        Ok(self.repo.get_object_meta(namespace_id, key).map(Into::into))
    }

    async fn list_objects(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<CoreObjectMeta>, ServerCoreError> {
        Ok(self
            .repo
            .list_objects(namespace_id, limit, offset)
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn delete_object(&self, namespace_id: &str, key: &str) -> Result<(), ServerCoreError> {
        self.repo
            .delete_object(namespace_id, key)
            .map_err(ServerCoreError::from)
    }

    async fn count_refs_to_blob(
        &self,
        namespace_id: &str,
        blob_key: &str,
    ) -> Result<u64, ServerCoreError> {
        Ok(self.repo.count_refs_to_blob(namespace_id, blob_key))
    }

    async fn record_usage(
        &self,
        user_id: &str,
        event_type: &str,
        amount: u64,
        namespace_id: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        self.repo
            .record_usage(user_id, event_type, amount, namespace_id)
            .map_err(ServerCoreError::from)
    }

    async fn get_usage_totals(&self, user_id: &str) -> Result<CoreUsageTotals, ServerCoreError> {
        Ok(self.repo.get_usage_totals(user_id).into())
    }

    async fn get_namespace_usage_totals(
        &self,
        user_id: &str,
        namespace_id: &str,
    ) -> Result<CoreUsageTotals, ServerCoreError> {
        Ok(self
            .repo
            .get_namespace_usage_totals(user_id, namespace_id)
            .into())
    }
}

impl From<crate::db::NamespaceObjectMeta> for CoreObjectMeta {
    fn from(value: crate::db::NamespaceObjectMeta) -> Self {
        Self {
            namespace_id: value.namespace_id,
            key: value.key,
            blob_key: value.r2_key,
            mime_type: value.mime_type,
            size_bytes: value.size_bytes,
            updated_at: value.updated_at,
            audience: value.audience,
            content_hash: value.content_hash,
        }
    }
}

impl From<crate::db::UsageTotals> for CoreUsageTotals {
    fn from(value: crate::db::UsageTotals) -> Self {
        Self {
            bytes_in: value.bytes_in,
            bytes_out: value.bytes_out,
            relay_seconds: value.relay_seconds,
        }
    }
}

impl From<crate::db::UserTier> for CoreUserTier {
    fn from(value: crate::db::UserTier) -> Self {
        match value {
            crate::db::UserTier::Free => CoreUserTier::Free,
            crate::db::UserTier::Plus => CoreUserTier::Plus,
        }
    }
}

impl From<crate::db::UserInfo> for CoreUserInfo {
    fn from(value: crate::db::UserInfo) -> Self {
        Self {
            id: value.id,
            email: value.email,
            created_at: value.created_at,
            last_login_at: value.last_login_at,
            attachment_limit_bytes: value.attachment_limit_bytes,
            workspace_limit: value.workspace_limit,
            tier: value.tier.into(),
            published_site_limit: value.published_site_limit,
        }
    }
}

impl From<crate::db::DeviceInfo> for CoreDeviceInfo {
    fn from(value: crate::db::DeviceInfo) -> Self {
        Self {
            id: value.id,
            user_id: value.user_id,
            name: value.name,
            user_agent: value.user_agent,
            created_at: value.created_at,
            last_seen_at: value.last_seen_at,
        }
    }
}

impl From<crate::db::NamespaceInfo> for CoreNamespaceInfo {
    fn from(value: crate::db::NamespaceInfo) -> Self {
        Self {
            id: value.id,
            owner_user_id: value.owner_user_id,
            created_at: value.created_at,
        }
    }
}

impl From<crate::db::AudienceInfo> for CoreAudienceInfo {
    fn from(value: crate::db::AudienceInfo) -> Self {
        Self {
            namespace_id: value.namespace_id,
            audience_name: value.audience_name,
            access: value.access,
        }
    }
}

impl From<crate::db::NamespaceSessionInfo> for CoreNamespaceSessionInfo {
    fn from(value: crate::db::NamespaceSessionInfo) -> Self {
        Self {
            code: value.code,
            namespace_id: value.namespace_id,
            owner_user_id: value.owner_user_id,
            read_only: value.read_only,
            created_at: value.created_at,
            expires_at: value.expires_at,
        }
    }
}

impl From<crate::db::CustomDomainInfo> for CoreCustomDomainInfo {
    fn from(value: crate::db::CustomDomainInfo) -> Self {
        Self {
            domain: value.domain,
            namespace_id: value.namespace_id,
            audience_name: value.audience_name,
            created_at: value.created_at,
            verified: value.verified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeAuthStore, NativeDomainMappingCache, NativeNamespaceStore, NativeSessionStore,
    };
    use crate::db::{AuthRepo, NamespaceRepo, init_database};
    use diaryx_server::ports::DomainMappingCache;
    use diaryx_server::use_cases::current_user::CurrentUserService;
    use diaryx_server::use_cases::domains::DomainService;
    use diaryx_server::use_cases::namespaces::NamespaceService;
    use diaryx_server::use_cases::sessions::SessionService;
    use rusqlite::Connection;
    use std::sync::Arc;

    #[tokio::test]
    async fn native_adapters_support_current_user_use_case() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_database(&conn).expect("schema");

        let repo = Arc::new(AuthRepo::new(conn));
        let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));

        let user_id = repo
            .get_or_create_user("user@example.com")
            .expect("user created");
        repo.create_device(&user_id, Some("Laptop"), Some("test-agent"))
            .expect("device created");
        ns_repo
            .create_namespace("workspace:test", &user_id)
            .expect("namespace created");

        let auth_store = NativeAuthStore::new(repo);
        let namespace_store = NativeNamespaceStore::new(ns_repo);
        let service = CurrentUserService::new(&auth_store, &namespace_store);

        let context = service
            .load(&user_id, "fallback@example.com")
            .await
            .expect("current user loaded");

        assert_eq!(context.user.email, "user@example.com");
        assert_eq!(context.devices.len(), 1);
        assert_eq!(context.namespaces.len(), 1);
        assert_eq!(context.namespaces[0].id, "workspace:test");
    }

    #[tokio::test]
    async fn native_namespace_store_supports_domain_service_use_case() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_database(&conn).expect("schema");

        let repo = Arc::new(AuthRepo::new(conn));
        let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));

        let user_id = repo
            .get_or_create_user("user@example.com")
            .expect("user created");
        ns_repo
            .create_namespace("workspace:test", &user_id)
            .expect("namespace created");
        ns_repo
            .upsert_audience("workspace:test", "public", "public")
            .expect("audience created");

        let namespace_store = NativeNamespaceStore::new(ns_repo);
        let cache = NativeDomainMappingCache::new(reqwest::Client::new(), "", None, None);
        let service = DomainService::new(&namespace_store, &cache);

        let domain = service
            .register_domain("workspace:test", "blog.example.com", "public")
            .await
            .expect("domain registered");
        assert_eq!(domain.audience_name, "public");

        service
            .remove_domain("workspace:test", "blog.example.com")
            .await
            .expect("domain removed");
    }

    #[tokio::test]
    async fn native_namespace_store_supports_namespace_service_use_case() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_database(&conn).expect("schema");

        let repo = Arc::new(AuthRepo::new(conn));
        let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));

        let user_id = repo
            .get_or_create_user("user@example.com")
            .expect("user created");

        let namespace_store = NativeNamespaceStore::new(ns_repo);
        let service = NamespaceService::new(&namespace_store);

        let ns = service
            .create(&user_id, Some("workspace:test"))
            .await
            .expect("namespace created");
        assert_eq!(ns.id, "workspace:test");

        let fetched = service.get("workspace:test", &user_id).await.expect("get");
        assert_eq!(fetched.owner_user_id, user_id);

        service
            .delete("workspace:test", &user_id)
            .await
            .expect("deleted");
    }

    #[tokio::test]
    async fn native_session_store_supports_session_service_use_case() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_database(&conn).expect("schema");

        let repo = Arc::new(AuthRepo::new(conn));
        let ns_repo = Arc::new(NamespaceRepo::new(repo.connection()));

        let user_id = repo
            .get_or_create_user("user@example.com")
            .expect("user created");
        ns_repo
            .create_namespace("workspace:test", &user_id)
            .expect("namespace created");

        let namespace_store = NativeNamespaceStore::new(ns_repo.clone());
        let session_store = NativeSessionStore::new(ns_repo);
        let service = SessionService::new(&namespace_store, &session_store);

        let session = service
            .create("workspace:test", &user_id, false)
            .await
            .expect("session created");
        assert_eq!(session.namespace_id, "workspace:test");

        let fetched = service.get(&session.code).await.expect("get");
        assert_eq!(fetched.namespace_id, "workspace:test");

        service
            .delete(&session.code, &user_id)
            .await
            .expect("deleted");
    }

    #[tokio::test]
    async fn native_domain_mapping_cache_is_a_noop_without_cloudflare_config() {
        let cache = NativeDomainMappingCache::new(reqwest::Client::new(), "", None, None);

        cache
            .put_domain("blog.example.com", "ns_123", "public")
            .await
            .expect("no-op put");
        cache
            .put_subdomain("notes", "ns_123", Some("public"))
            .await
            .expect("no-op put");
        cache
            .delete_domain("blog.example.com")
            .await
            .expect("no-op delete");
        cache.delete_subdomain("notes").await.expect("no-op delete");
    }
}
