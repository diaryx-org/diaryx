use crate::domain::AudienceInfo;
use crate::ports::{BlobStore, NamespaceStore, ServerCoreError};
use tracing::warn;

const VALID_ACCESS_LEVELS: &[&str] = &["public", "token", "private"];

pub struct AudienceService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    blob_store: &'a dyn BlobStore,
}

impl<'a> AudienceService<'a> {
    pub fn new(namespace_store: &'a dyn NamespaceStore, blob_store: &'a dyn BlobStore) -> Self {
        Self {
            namespace_store,
            blob_store,
        }
    }

    fn require_owner<'b>(
        &self,
        ns: &'b crate::domain::NamespaceInfo,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }
        Ok(())
    }

    async fn require_namespace_owner(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        let ns = self
            .namespace_store
            .get_namespace(namespace_id)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Namespace not found"))?;
        self.require_owner(&ns, caller_user_id)
    }

    pub async fn set(
        &self,
        namespace_id: &str,
        audience_name: &str,
        access: &str,
        caller_user_id: &str,
    ) -> Result<AudienceInfo, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        if !VALID_ACCESS_LEVELS.contains(&access) {
            return Err(ServerCoreError::invalid_input(
                "access must be 'public', 'token', or 'private'",
            ));
        }

        self.namespace_store
            .upsert_audience(namespace_id, audience_name, access)
            .await?;

        let info = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::internal("Audience missing after upsert"))?;

        self.write_audiences_meta(namespace_id).await;
        Ok(info)
    }

    pub async fn list(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<Vec<AudienceInfo>, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;
        self.namespace_store.list_audiences(namespace_id).await
    }

    pub async fn delete(
        &self,
        namespace_id: &str,
        audience_name: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        if self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .is_none()
        {
            return Err(ServerCoreError::not_found("Audience not found"));
        }

        self.namespace_store
            .clear_objects_audience(namespace_id, audience_name)
            .await?;
        self.namespace_store
            .delete_audience(namespace_id, audience_name)
            .await?;
        self.write_audiences_meta(namespace_id).await;
        Ok(())
    }

    /// Check whether an audience is eligible for token generation.
    /// Returns the audience info on success, or an error if the audience
    /// is public or does not exist.
    pub async fn require_token_eligible(
        &self,
        namespace_id: &str,
        audience_name: &str,
        caller_user_id: &str,
    ) -> Result<AudienceInfo, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let audience = self
            .namespace_store
            .get_audience(namespace_id, audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Audience not found"))?;

        if audience.access == "public" {
            return Err(ServerCoreError::invalid_input(
                "audience is public; no token needed",
            ));
        }

        Ok(audience)
    }

    /// Write `ns/{ns_id}/_audiences.json` to the blob store.
    /// Best-effort — errors are logged but do not fail the caller.
    async fn write_audiences_meta(&self, namespace_id: &str) {
        let audiences = match self.namespace_store.list_audiences(namespace_id).await {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    "Failed to list audiences for metadata write ({}): {}",
                    namespace_id, e
                );
                return;
            }
        };

        let map: serde_json::Map<String, serde_json::Value> = audiences
            .into_iter()
            .map(|a| (a.audience_name, serde_json::Value::String(a.access)))
            .collect();

        let json = serde_json::to_vec(&map).unwrap_or_default();
        let key = format!("ns/{}/_audiences.json", namespace_id);
        if let Err(e) = self
            .blob_store
            .put(&key, &json, "application/json", None)
            .await
        {
            warn!(
                "Failed to write audiences metadata for {}: {}",
                namespace_id, e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AudienceService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo};
    use crate::ports::{BlobStore, MultipartCompletedPart, NamespaceStore, ServerCoreError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
        audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
    }

    crate::cfg_async_trait! {
    impl NamespaceStore for TestStore {
        async fn get_namespace(
            &self,
            namespace_id: &str,
        ) -> Result<Option<NamespaceInfo>, ServerCoreError> {
            Ok(self.namespaces.lock().unwrap().get(namespace_id).cloned())
        }
        async fn list_namespaces(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
            Ok(vec![])
        }
        async fn create_namespace(&self, _: &str, _: &str, _: Option<&str>) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn update_namespace_metadata(&self, _: &str, _: Option<&str>) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_namespace(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .unwrap()
                .get(&(namespace_id.to_string(), audience_name.to_string()))
                .cloned())
        }
        async fn upsert_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
            access: &str,
        ) -> Result<(), ServerCoreError> {
            self.audiences.lock().unwrap().insert(
                (namespace_id.to_string(), audience_name.to_string()),
                AudienceInfo {
                    namespace_id: namespace_id.to_string(),
                    audience_name: audience_name.to_string(),
                    access: access.to_string(),
                },
            );
            Ok(())
        }
        async fn list_audiences(
            &self,
            namespace_id: &str,
        ) -> Result<Vec<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .unwrap()
                .values()
                .filter(|a| a.namespace_id == namespace_id)
                .cloned()
                .collect())
        }
        async fn delete_audience(
            &self,
            namespace_id: &str,
            audience_name: &str,
        ) -> Result<(), ServerCoreError> {
            self.audiences
                .lock()
                .unwrap()
                .remove(&(namespace_id.to_string(), audience_name.to_string()));
            Ok(())
        }
        async fn clear_objects_audience(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
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
    }
    }

    #[derive(Default)]
    struct TestBlobStore {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
    }

    crate::cfg_async_trait! {
    impl BlobStore for TestBlobStore {
        fn blob_key(&self, _: &str, _: &str) -> String {
            String::new()
        }
        fn prefix(&self) -> &str {
            ""
        }
        async fn put(
            &self,
            key: &str,
            bytes: &[u8],
            _: &str,
            _: Option<&HashMap<String, String>>,
        ) -> Result<(), ServerCoreError> {
            self.blobs
                .lock()
                .unwrap()
                .insert(key.to_string(), bytes.to_vec());
            Ok(())
        }
        async fn get(&self, _: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(None)
        }
        async fn delete(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn exists(&self, _: &str) -> Result<bool, ServerCoreError> {
            Ok(false)
        }
        async fn init_multipart(&self, _: &str, _: &str) -> Result<String, ServerCoreError> {
            Ok(String::new())
        }
        async fn upload_part(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &[u8],
        ) -> Result<String, ServerCoreError> {
            Ok(String::new())
        }
        async fn complete_multipart(
            &self,
            _: &str,
            _: &str,
            _: &[MultipartCompletedPart],
        ) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn abort_multipart(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_range(
            &self,
            _: &str,
            _: u64,
            _: u64,
        ) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(None)
        }
        async fn list_by_prefix(&self, _: &str) -> Result<Vec<String>, ServerCoreError> {
            Ok(vec![])
        }
        async fn delete_by_prefix(&self, _: &str) -> Result<usize, ServerCoreError> {
            Ok(0)
        }
    }
    }

    fn make_store_with_namespace(owner: &str, ns_id: &str) -> TestStore {
        let store = TestStore::default();
        store.namespaces.lock().unwrap().insert(
            ns_id.to_string(),
            NamespaceInfo {
                id: ns_id.to_string(),
                owner_user_id: owner.to_string(),
                created_at: 1,
                metadata: None,
            },
        );
        store
    }

    #[tokio::test]
    async fn set_audience_validates_access() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let err = service
            .set("ns1", "public", "invalid", "user1")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn set_and_list_audiences() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set("ns1", "public", "public", "user1")
            .await
            .unwrap();
        service
            .set("ns1", "members", "token", "user1")
            .await
            .unwrap();

        let audiences = service.list("ns1", "user1").await.unwrap();
        assert_eq!(audiences.len(), 2);

        // Check that metadata blob was written
        assert!(
            blob_store
                .blobs
                .lock()
                .unwrap()
                .contains_key("ns/ns1/_audiences.json")
        );
    }

    #[tokio::test]
    async fn delete_audience_clears_objects_and_writes_meta() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set("ns1", "public", "public", "user1")
            .await
            .unwrap();
        service.delete("ns1", "public", "user1").await.unwrap();

        let audiences = service.list("ns1", "user1").await.unwrap();
        assert!(audiences.is_empty());
    }

    #[tokio::test]
    async fn require_token_eligible_rejects_public() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service.set("ns1", "pub", "public", "user1").await.unwrap();

        let err = service
            .require_token_eligible("ns1", "pub", "user1")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn require_token_eligible_accepts_token_audience() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        service
            .set("ns1", "members", "token", "user1")
            .await
            .unwrap();

        let info = service
            .require_token_eligible("ns1", "members", "user1")
            .await
            .unwrap();
        assert_eq!(info.access, "token");
    }

    #[tokio::test]
    async fn rejects_non_owner() {
        let store = make_store_with_namespace("user1", "ns1");
        let blob_store = TestBlobStore::default();
        let service = AudienceService::new(&store, &blob_store);

        let err = service
            .set("ns1", "public", "public", "user2")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }
}
