use crate::domain::{CurrentUserContext, TierDefaults};
use crate::ports::{AuthStore, NamespaceStore, ServerCoreError};

pub struct CurrentUserService<'a> {
    auth_store: &'a dyn AuthStore,
    namespace_store: &'a dyn NamespaceStore,
}

impl<'a> CurrentUserService<'a> {
    pub fn new(auth_store: &'a dyn AuthStore, namespace_store: &'a dyn NamespaceStore) -> Self {
        Self {
            auth_store,
            namespace_store,
        }
    }

    pub async fn load(
        &self,
        user_id: &str,
        fallback_email: &str,
    ) -> Result<CurrentUserContext, ServerCoreError> {
        let user =
            self.auth_store.get_user(user_id).await?.ok_or_else(|| {
                ServerCoreError::not_found(format!("User '{}' not found", user_id))
            })?;
        let devices = self.auth_store.list_user_devices(user_id).await?;
        let namespaces = self
            .namespace_store
            .list_namespaces(user_id, 100, 0)
            .await?;

        let defaults = user.tier.defaults();
        let limits = TierDefaults {
            device_limit: defaults.device_limit,
            attachment_limit_bytes: user
                .attachment_limit_bytes
                .unwrap_or(defaults.attachment_limit_bytes),
            workspace_limit: user.workspace_limit.unwrap_or(defaults.workspace_limit),
            published_site_limit: user
                .published_site_limit
                .unwrap_or(defaults.published_site_limit),
        };

        let mut user = user;
        if user.email.is_empty() {
            user.email = fallback_email.to_string();
        }

        Ok(CurrentUserContext {
            user,
            devices,
            namespaces,
            limits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::CurrentUserService;
    use crate::domain::{DeviceInfo, NamespaceInfo, UserInfo, UserTier};
    use crate::ports::{AuthStore, NamespaceStore, ServerCoreError};
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};

    struct TestAuthStore;
    struct TestNamespaceStore;

    #[async_trait]
    impl AuthStore for TestAuthStore {
        async fn get_user(&self, user_id: &str) -> Result<Option<UserInfo>, ServerCoreError> {
            Ok(Some(UserInfo {
                id: user_id.to_string(),
                email: "user@example.com".to_string(),
                created_at: Utc.timestamp_opt(1, 0).unwrap(),
                last_login_at: None,
                attachment_limit_bytes: None,
                workspace_limit: Some(4),
                tier: UserTier::Plus,
                published_site_limit: None,
            }))
        }

        async fn list_user_devices(
            &self,
            user_id: &str,
        ) -> Result<Vec<DeviceInfo>, ServerCoreError> {
            Ok(vec![DeviceInfo {
                id: "dev1".to_string(),
                user_id: user_id.to_string(),
                name: Some("Laptop".to_string()),
                user_agent: None,
                created_at: Utc.timestamp_opt(1, 0).unwrap(),
                last_seen_at: Utc.timestamp_opt(2, 0).unwrap(),
            }])
        }

        async fn rename_device(
            &self,
            _device_id: &str,
            _new_name: &str,
        ) -> Result<bool, ServerCoreError> {
            Ok(true)
        }

        async fn delete_device(&self, _device_id: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }

        async fn get_user_tier(&self, _user_id: &str) -> Result<UserTier, ServerCoreError> {
            Ok(UserTier::Plus)
        }
    }

    #[async_trait]
    impl NamespaceStore for TestNamespaceStore {
        async fn get_namespace(
            &self,
            _namespace_id: &str,
        ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
            Ok(None)
        }

        async fn list_namespaces(
            &self,
            owner_user_id: &str,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
            Ok(vec![NamespaceInfo {
                id: "workspace:test".to_string(),
                owner_user_id: owner_user_id.to_string(),
                created_at: 1,
            }])
        }

        async fn get_audience(
            &self,
            _namespace_id: &str,
            _audience_name: &str,
        ) -> Result<Option<crate::domain::AudienceInfo>, ServerCoreError> {
            Ok(None)
        }

        async fn get_custom_domain(
            &self,
            _domain: &str,
        ) -> Result<Option<crate::domain::CustomDomainInfo>, ServerCoreError> {
            Ok(None)
        }

        async fn list_custom_domains(
            &self,
            _namespace_id: &str,
        ) -> Result<Vec<crate::domain::CustomDomainInfo>, ServerCoreError> {
            Ok(vec![])
        }

        async fn upsert_custom_domain(
            &self,
            _domain: &str,
            _namespace_id: &str,
            _audience_name: &str,
        ) -> Result<(), ServerCoreError> {
            Ok(())
        }

        async fn delete_custom_domain(&self, _domain: &str) -> Result<bool, ServerCoreError> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn current_user_service_builds_effective_limits() {
        let auth_store = TestAuthStore;
        let namespace_store = TestNamespaceStore;
        let service = CurrentUserService::new(&auth_store, &namespace_store);

        let result = service.load("u1", "fallback@example.com").await.unwrap();
        assert_eq!(result.user.id, "u1");
        assert_eq!(result.devices.len(), 1);
        assert_eq!(result.namespaces.len(), 1);
        assert_eq!(result.limits.workspace_limit, 4);
        assert_eq!(result.limits.device_limit, 10);
    }
}
