use crate::domain::NamespaceInfo;
use crate::ports::{NamespaceStore, ServerCoreError};
use uuid::Uuid;

pub struct NamespaceService<'a> {
    namespace_store: &'a dyn NamespaceStore,
}

impl<'a> NamespaceService<'a> {
    pub fn new(namespace_store: &'a dyn NamespaceStore) -> Self {
        Self { namespace_store }
    }

    pub async fn create(
        &self,
        owner_user_id: &str,
        id: Option<&str>,
        metadata: Option<&str>,
    ) -> Result<NamespaceInfo, ServerCoreError> {
        let namespace_id = id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        self.namespace_store
            .create_namespace(&namespace_id, owner_user_id, metadata)
            .await
            .map_err(|_| ServerCoreError::conflict("Namespace already exists"))?;

        self.namespace_store
            .get_namespace(&namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::internal("Namespace was missing after creation"))
    }

    pub async fn update_metadata(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
        metadata: Option<&str>,
    ) -> Result<NamespaceInfo, ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;

        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }

        self.namespace_store
            .update_namespace_metadata(namespace_id, metadata)
            .await?;

        self.namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::internal("Namespace missing after metadata update"))
    }

    pub async fn get(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<NamespaceInfo, ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;

        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }

        Ok(ns)
    }

    pub async fn list(
        &self,
        owner_user_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
        let limit = limit.min(500);
        self.namespace_store
            .list_namespaces(owner_user_id, limit, offset)
            .await
    }

    pub async fn delete(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;

        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }

        self.namespace_store.delete_namespace(namespace_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::NamespaceService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo};
    use crate::ports::{NamespaceStore, ServerCoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
    }

    crate::cfg_async_trait! {
    impl NamespaceStore for TestNamespaceStore {
        async fn get_namespace(
            &self,
            namespace_id: &str,
        ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
            Ok(self.namespaces.lock().unwrap().get(namespace_id).cloned())
        }

        async fn list_namespaces(
            &self,
            owner_user_id: &str,
            limit: u32,
            _offset: u32,
        ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
            Ok(self
                .namespaces
                .lock()
                .unwrap()
                .values()
                .filter(|ns| ns.owner_user_id == owner_user_id)
                .take(limit as usize)
                .cloned()
                .collect())
        }

        async fn create_namespace(
            &self,
            namespace_id: &str,
            owner_user_id: &str,
            metadata: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            let mut map = self.namespaces.lock().unwrap();
            if map.contains_key(namespace_id) {
                return Err(ServerCoreError::conflict("already exists"));
            }
            map.insert(
                namespace_id.to_string(),
                NamespaceInfo {
                    id: namespace_id.to_string(),
                    owner_user_id: owner_user_id.to_string(),
                    created_at: 1,
                    metadata: metadata.map(String::from),
                },
            );
            Ok(())
        }

        async fn update_namespace_metadata(
            &self,
            namespace_id: &str,
            metadata: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            let mut map = self.namespaces.lock().unwrap();
            if let Some(ns) = map.get_mut(namespace_id) {
                ns.metadata = metadata.map(String::from);
            }
            Ok(())
        }

        async fn delete_namespace(&self, namespace_id: &str) -> Result<(), ServerCoreError> {
            self.namespaces.lock().unwrap().remove(namespace_id);
            Ok(())
        }

        async fn get_audience(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(None)
        }
        async fn get_custom_domain(
            &self,
            _: &str,
        ) -> Result<Option<CustomDomainInfo>, ServerCoreError> {
            Ok(None)
        }
        async fn list_custom_domains(
            &self,
            _: &str,
        ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn upsert_custom_domain(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_custom_domain(&self, _: &str) -> Result<bool, ServerCoreError> {
            Ok(false)
        }
        async fn upsert_audience(&self, _: &str, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn list_audiences(&self, _: &str) -> Result<Vec<AudienceInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn delete_audience(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn clear_objects_audience(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
    }
    }

    #[tokio::test]
    async fn create_and_get_namespace() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        let ns = service
            .create("user1", Some("workspace:abc"), None)
            .await
            .unwrap();
        assert_eq!(ns.id, "workspace:abc");
        assert_eq!(ns.owner_user_id, "user1");

        let fetched = service.get("workspace:abc", "user1").await.unwrap();
        assert_eq!(fetched.id, "workspace:abc");
    }

    #[tokio::test]
    async fn create_generates_uuid_when_no_id() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        let ns = service.create("user1", None, None).await.unwrap();
        assert!(!ns.id.is_empty());
        assert_eq!(ns.owner_user_id, "user1");
    }

    #[tokio::test]
    async fn get_rejects_non_owner() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        service
            .create("user1", Some("workspace:abc"), None)
            .await
            .unwrap();

        let err = service.get("workspace:abc", "user2").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn delete_rejects_non_owner() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        service
            .create("user1", Some("workspace:abc"), None)
            .await
            .unwrap();

        let err = service.delete("workspace:abc", "user2").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn delete_removes_namespace() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        service
            .create("user1", Some("workspace:abc"), None)
            .await
            .unwrap();
        service.delete("workspace:abc", "user1").await.unwrap();

        let err = service.get("workspace:abc", "user1").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_caps_limit_at_500() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        let result = service.list("user1", 9999, 0).await.unwrap();
        assert!(result.is_empty());
    }
}
