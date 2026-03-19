use crate::domain::{ObjectMeta, PublicObjectAccess, UsageTotals};
use crate::ports::{BlobStore, NamespaceStore, ObjectMetaStore, ServerCoreError};
use std::collections::HashMap;

/// Result of a successful object put.
#[derive(Debug, Clone)]
pub struct PutObjectResult {
    pub key: String,
    pub size_bytes: u64,
}

/// Result of a successful authenticated object get.
#[derive(Debug)]
pub struct GetObjectResult {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub struct ObjectService<'a> {
    namespace_store: &'a dyn NamespaceStore,
    object_meta_store: &'a dyn ObjectMetaStore,
    blob_store: &'a dyn BlobStore,
}

impl<'a> ObjectService<'a> {
    pub fn new(
        namespace_store: &'a dyn NamespaceStore,
        object_meta_store: &'a dyn ObjectMetaStore,
        blob_store: &'a dyn BlobStore,
    ) -> Self {
        Self {
            namespace_store,
            object_meta_store,
            blob_store,
        }
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
        if ns.owner_user_id != caller_user_id {
            return Err(ServerCoreError::permission_denied(
                "You do not own this namespace",
            ));
        }
        Ok(())
    }

    pub async fn put(
        &self,
        namespace_id: &str,
        key: &str,
        mime_type: &str,
        bytes: &[u8],
        audience: Option<&str>,
        caller_user_id: &str,
    ) -> Result<PutObjectResult, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        // Validate audience exists if specified.
        if let Some(aud) = audience {
            if self
                .namespace_store
                .get_audience(namespace_id, aud)
                .await?
                .is_none()
            {
                return Err(ServerCoreError::invalid_input(format!(
                    "audience '{}' does not exist",
                    aud
                )));
            }
        }

        let blob_key = object_blob_key(namespace_id, key);
        let size = bytes.len() as u64;

        // Build R2 metadata with audience info.
        let r2_metadata = match audience {
            Some(aud) => {
                let mut m = HashMap::new();
                m.insert("audience".to_string(), aud.to_string());
                if let Some(info) = self.namespace_store.get_audience(namespace_id, aud).await? {
                    m.insert("access".to_string(), info.access);
                }
                Some(m)
            }
            None => None,
        };

        self.blob_store
            .put(&blob_key, bytes, mime_type, r2_metadata.as_ref())
            .await?;

        self.object_meta_store
            .upsert_object(namespace_id, key, &blob_key, mime_type, size, audience)
            .await?;

        // Record bytes_in usage (fire-and-forget; errors are non-fatal).
        let _ = self
            .object_meta_store
            .record_usage(caller_user_id, "bytes_in", size, Some(namespace_id))
            .await;

        Ok(PutObjectResult {
            key: key.to_string(),
            size_bytes: size,
        })
    }

    pub async fn get(
        &self,
        namespace_id: &str,
        key: &str,
        caller_user_id: &str,
    ) -> Result<GetObjectResult, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let meta = self
            .object_meta_store
            .get_object_meta(namespace_id, key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        let blob_key = meta
            .blob_key
            .unwrap_or_else(|| object_blob_key(namespace_id, key));

        let bytes = self
            .blob_store
            .get(&blob_key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        let size = bytes.len() as u64;
        let _ = self
            .object_meta_store
            .record_usage(caller_user_id, "bytes_out", size, Some(namespace_id))
            .await;

        Ok(GetObjectResult {
            mime_type: meta.mime_type,
            bytes,
        })
    }

    pub async fn delete(
        &self,
        namespace_id: &str,
        key: &str,
        caller_user_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let meta = self
            .object_meta_store
            .get_object_meta(namespace_id, key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        let blob_key = meta
            .blob_key
            .unwrap_or_else(|| object_blob_key(namespace_id, key));

        self.blob_store.delete(&blob_key).await?;
        self.object_meta_store
            .delete_object(namespace_id, key)
            .await?;

        Ok(())
    }

    pub async fn list(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
        caller_user_id: &str,
    ) -> Result<Vec<ObjectMeta>, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let limit = limit.min(500);
        self.object_meta_store
            .list_objects(namespace_id, limit, offset)
            .await
    }

    pub async fn get_usage(&self, user_id: &str) -> Result<UsageTotals, ServerCoreError> {
        self.object_meta_store.get_usage_totals(user_id).await
    }

    pub async fn get_namespace_usage(
        &self,
        namespace_id: &str,
        caller_user_id: &str,
    ) -> Result<UsageTotals, ServerCoreError> {
        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        self.object_meta_store
            .get_namespace_usage_totals(caller_user_id, namespace_id)
            .await
    }

    /// Resolve the access level for a public (unauthenticated) object request.
    /// Returns the object metadata and audience info needed for the handler
    /// to perform token validation and serve the blob.
    pub async fn resolve_public_access(
        &self,
        namespace_id: &str,
        key: &str,
    ) -> Result<PublicObjectAccess, ServerCoreError> {
        let meta = self
            .object_meta_store
            .get_object_meta(namespace_id, key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        let audience_name = meta
            .audience
            .as_ref()
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?
            .clone();

        let audience = self
            .namespace_store
            .get_audience(namespace_id, &audience_name)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        Ok(PublicObjectAccess {
            meta,
            access: audience.access,
            audience_name,
        })
    }

    /// Fetch bytes from the blob store for a resolved public object.
    pub async fn fetch_blob(
        &self,
        namespace_id: &str,
        key: &str,
        blob_key: Option<&str>,
    ) -> Result<GetObjectResult, ServerCoreError> {
        let blob_key = blob_key
            .map(|k| k.to_string())
            .unwrap_or_else(|| object_blob_key(namespace_id, key));

        let meta = self
            .object_meta_store
            .get_object_meta(namespace_id, key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        let bytes = self
            .blob_store
            .get(&blob_key)
            .await?
            .ok_or_else(|| ServerCoreError::not_found("Object not found"))?;

        Ok(GetObjectResult {
            mime_type: meta.mime_type,
            bytes,
        })
    }
}

/// Derive the blob store key for a namespace object.
pub fn object_blob_key(namespace_id: &str, key: &str) -> String {
    format!("ns/{}/{}", namespace_id, key)
}

#[cfg(test)]
mod tests {
    use super::ObjectService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo, ObjectMeta, UsageTotals};
    use crate::ports::{
        BlobStore, MultipartCompletedPart, NamespaceStore, ObjectMetaStore, ServerCoreError,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
        audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
    }

    #[async_trait]
    impl NamespaceStore for TestNamespaceStore {
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
        async fn create_namespace(&self, _: &str, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn delete_namespace(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }
        async fn get_audience(
            &self,
            ns: &str,
            name: &str,
        ) -> Result<Option<AudienceInfo>, ServerCoreError> {
            Ok(self
                .audiences
                .lock()
                .unwrap()
                .get(&(ns.to_string(), name.to_string()))
                .cloned())
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

    #[derive(Default)]
    struct TestObjectMetaStore {
        objects: Mutex<HashMap<(String, String), ObjectMeta>>,
        usage: Mutex<Vec<(String, String, u64)>>,
    }

    #[async_trait]
    impl ObjectMetaStore for TestObjectMetaStore {
        async fn upsert_object(
            &self,
            namespace_id: &str,
            key: &str,
            blob_key: &str,
            mime_type: &str,
            size_bytes: u64,
            audience: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            self.objects.lock().unwrap().insert(
                (namespace_id.to_string(), key.to_string()),
                ObjectMeta {
                    namespace_id: namespace_id.to_string(),
                    key: key.to_string(),
                    blob_key: Some(blob_key.to_string()),
                    mime_type: mime_type.to_string(),
                    size_bytes,
                    updated_at: 1,
                    audience: audience.map(|s| s.to_string()),
                },
            );
            Ok(())
        }
        async fn get_object_meta(
            &self,
            namespace_id: &str,
            key: &str,
        ) -> Result<Option<ObjectMeta>, ServerCoreError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .get(&(namespace_id.to_string(), key.to_string()))
                .cloned())
        }
        async fn list_objects(
            &self,
            namespace_id: &str,
            limit: u32,
            _offset: u32,
        ) -> Result<Vec<ObjectMeta>, ServerCoreError> {
            Ok(self
                .objects
                .lock()
                .unwrap()
                .values()
                .filter(|o| o.namespace_id == namespace_id)
                .take(limit as usize)
                .cloned()
                .collect())
        }
        async fn delete_object(
            &self,
            namespace_id: &str,
            key: &str,
        ) -> Result<(), ServerCoreError> {
            self.objects
                .lock()
                .unwrap()
                .remove(&(namespace_id.to_string(), key.to_string()));
            Ok(())
        }
        async fn record_usage(
            &self,
            user_id: &str,
            event_type: &str,
            amount: u64,
            _namespace_id: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            self.usage
                .lock()
                .unwrap()
                .push((user_id.to_string(), event_type.to_string(), amount));
            Ok(())
        }
        async fn get_usage_totals(&self, _user_id: &str) -> Result<UsageTotals, ServerCoreError> {
            Ok(UsageTotals::default())
        }
        async fn get_namespace_usage_totals(
            &self,
            _user_id: &str,
            _namespace_id: &str,
        ) -> Result<UsageTotals, ServerCoreError> {
            Ok(UsageTotals::default())
        }
    }

    #[derive(Default)]
    struct TestBlobStore {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait]
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
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(self.blobs.lock().unwrap().get(key).cloned())
        }
        async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
            self.blobs.lock().unwrap().remove(key);
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

    fn make_stores() -> (TestNamespaceStore, TestObjectMetaStore, TestBlobStore) {
        let ns_store = TestNamespaceStore::default();
        ns_store.namespaces.lock().unwrap().insert(
            "ns1".to_string(),
            NamespaceInfo {
                id: "ns1".to_string(),
                owner_user_id: "user1".to_string(),
                created_at: 1,
            },
        );
        (
            ns_store,
            TestObjectMetaStore::default(),
            TestBlobStore::default(),
        )
    }

    #[tokio::test]
    async fn put_and_get_object() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        let result = service
            .put(
                "ns1",
                "hello.txt",
                "text/plain",
                b"hello world",
                None,
                "user1",
            )
            .await
            .unwrap();
        assert_eq!(result.key, "hello.txt");
        assert_eq!(result.size_bytes, 11);

        let obj = service.get("ns1", "hello.txt", "user1").await.unwrap();
        assert_eq!(obj.mime_type, "text/plain");
        assert_eq!(obj.bytes, b"hello world");

        // Usage was recorded
        let usage = obj_store.usage.lock().unwrap();
        assert!(usage.iter().any(|(_, t, _)| t == "bytes_in"));
        assert!(usage.iter().any(|(_, t, _)| t == "bytes_out"));
    }

    #[tokio::test]
    async fn put_rejects_nonexistent_audience() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        let err = service
            .put(
                "ns1",
                "hello.txt",
                "text/plain",
                b"hello",
                Some("nonexistent"),
                "user1",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn put_with_valid_audience() {
        let (ns_store, obj_store, blob_store) = make_stores();
        ns_store.audiences.lock().unwrap().insert(
            ("ns1".to_string(), "public".to_string()),
            AudienceInfo {
                namespace_id: "ns1".to_string(),
                audience_name: "public".to_string(),
                access: "public".to_string(),
            },
        );
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        let result = service
            .put(
                "ns1",
                "page.html",
                "text/html",
                b"<h1>hi</h1>",
                Some("public"),
                "user1",
            )
            .await
            .unwrap();
        assert_eq!(result.key, "page.html");

        let meta = obj_store
            .objects
            .lock()
            .unwrap()
            .get(&("ns1".to_string(), "page.html".to_string()))
            .cloned()
            .unwrap();
        assert_eq!(meta.audience.as_deref(), Some("public"));
    }

    #[tokio::test]
    async fn delete_removes_blob_and_meta() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put("ns1", "hello.txt", "text/plain", b"hello", None, "user1")
            .await
            .unwrap();
        service.delete("ns1", "hello.txt", "user1").await.unwrap();

        assert!(
            obj_store
                .objects
                .lock()
                .unwrap()
                .get(&("ns1".to_string(), "hello.txt".to_string()))
                .is_none()
        );
        assert!(
            blob_store
                .blobs
                .lock()
                .unwrap()
                .get("ns/ns1/hello.txt")
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_non_owner() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        let err = service
            .put("ns1", "hello.txt", "text/plain", b"hello", None, "user2")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn resolve_public_access_for_public_audience() {
        let (ns_store, obj_store, blob_store) = make_stores();
        ns_store.audiences.lock().unwrap().insert(
            ("ns1".to_string(), "public".to_string()),
            AudienceInfo {
                namespace_id: "ns1".to_string(),
                audience_name: "public".to_string(),
                access: "public".to_string(),
            },
        );
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put(
                "ns1",
                "page.html",
                "text/html",
                b"<h1>hi</h1>",
                Some("public"),
                "user1",
            )
            .await
            .unwrap();

        let access = service
            .resolve_public_access("ns1", "page.html")
            .await
            .unwrap();
        assert_eq!(access.access, "public");
        assert_eq!(access.audience_name, "public");
    }

    #[tokio::test]
    async fn resolve_public_access_rejects_private_object() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put("ns1", "secret.txt", "text/plain", b"secret", None, "user1")
            .await
            .unwrap();

        let err = service
            .resolve_public_access("ns1", "secret.txt")
            .await
            .unwrap_err();
        assert!(matches!(err, ServerCoreError::NotFound(_)));
    }
}
