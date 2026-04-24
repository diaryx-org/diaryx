use crate::domain::NamespaceInfo;
use crate::ports::{DomainMappingCache, NamespaceStore, ServerCoreError};
use crate::use_cases::domains::DIARYX_SUBDOMAIN_SUFFIX;
use tracing::warn;
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
        self.delete_with_cache(namespace_id, caller_user_id, None)
            .await
    }

    /// Delete a namespace, cleaning up any domain/subdomain entries from the
    /// edge cache (Cloudflare KV) before the database CASCADE removes the rows.
    pub async fn delete_with_cache(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
        domain_cache: Option<&dyn DomainMappingCache>,
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

        // Clean up domain mapping cache entries before the CASCADE delete
        // removes the custom_domains rows we need to enumerate.
        if let Some(cache) = domain_cache {
            match self.namespace_store.list_custom_domains(namespace_id).await {
                Ok(domains) => {
                    for domain in &domains {
                        if domain.domain.ends_with(DIARYX_SUBDOMAIN_SUFFIX) {
                            let subdomain = domain.domain.trim_end_matches(DIARYX_SUBDOMAIN_SUFFIX);
                            if let Err(e) = cache.delete_subdomain(subdomain).await {
                                warn!(
                                    namespace_id,
                                    subdomain,
                                    "Failed to delete subdomain from cache during namespace cleanup: {e}"
                                );
                            }
                        } else if let Err(e) = cache.delete_domain(&domain.domain).await {
                            warn!(
                                namespace_id,
                                domain = %domain.domain,
                                "Failed to delete domain from cache during namespace cleanup: {e}"
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        namespace_id,
                        "Failed to list custom domains during namespace cleanup: {e}"
                    );
                }
            }
        }

        self.namespace_store.delete_namespace(namespace_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::NamespaceService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo};
    use crate::ports::{DomainMappingCache, NamespaceStore, ServerCoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
        custom_domains: Mutex<Vec<CustomDomainInfo>>,
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
            namespace_id: &str,
        ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
            Ok(self
                .custom_domains
                .lock()
                .unwrap()
                .iter()
                .filter(|d| d.namespace_id == namespace_id)
                .cloned()
                .collect())
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
        async fn upsert_audience(
            &self,
            _: &str,
            _: &str,
            _: &[crate::domain::GateRecord],
        ) -> Result<(), ServerCoreError> {
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

    #[derive(Default)]
    struct TestDomainMappingCache {
        deleted_domains: Mutex<Vec<String>>,
        deleted_subdomains: Mutex<Vec<String>>,
    }

    crate::cfg_async_trait! {
    impl DomainMappingCache for TestDomainMappingCache {
        async fn put_domain(&self, _: &str, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_domain(&self, hostname: &str) -> Result<(), ServerCoreError> {
            self.deleted_domains.lock().unwrap().push(hostname.to_string());
            Ok(())
        }
        async fn put_subdomain(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_subdomain(&self, subdomain: &str) -> Result<(), ServerCoreError> {
            self.deleted_subdomains.lock().unwrap().push(subdomain.to_string());
            Ok(())
        }
    }
    }

    #[tokio::test]
    async fn delete_with_cache_cleans_up_domains() {
        let store = TestNamespaceStore::default();
        let service = NamespaceService::new(&store);

        service.create("user1", Some("ns1"), None).await.unwrap();

        // Simulate a subdomain and a custom domain registered for this namespace.
        store.custom_domains.lock().unwrap().extend(vec![
            CustomDomainInfo {
                domain: "mysite.diaryx.org".to_string(),
                namespace_id: "ns1".to_string(),
                audience_name: "*".to_string(),
                created_at: 1,
                verified: false,
            },
            CustomDomainInfo {
                domain: "example.com".to_string(),
                namespace_id: "ns1".to_string(),
                audience_name: "public".to_string(),
                created_at: 1,
                verified: true,
            },
        ]);

        let cache = TestDomainMappingCache::default();
        service
            .delete_with_cache("ns1", "user1", Some(&cache))
            .await
            .unwrap();

        // Subdomain should be cleaned via delete_subdomain (label only, no suffix).
        let deleted_subdomains = cache.deleted_subdomains.lock().unwrap();
        assert_eq!(deleted_subdomains.len(), 1);
        assert_eq!(deleted_subdomains[0], "mysite");

        // Custom domain should be cleaned via delete_domain.
        let deleted_domains = cache.deleted_domains.lock().unwrap();
        assert_eq!(deleted_domains.len(), 1);
        assert_eq!(deleted_domains[0], "example.com");

        // Namespace should be gone.
        let err = service.get("ns1", "user1").await.unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }
}
