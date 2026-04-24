use crate::domain::{ObjectMeta, PublicObjectAccess, UsageTotals};
use crate::ports::{BlobStore, NamespaceStore, ObjectMetaStore, ServerCoreError};
use sha2::{Digest, Sha256};
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

/// Result of a batch object get.
#[derive(Debug)]
pub struct BatchGetResult {
    /// Successfully fetched objects keyed by their original key.
    pub objects: HashMap<String, GetObjectResult>,
    /// Per-key errors for objects that could not be fetched.
    pub errors: HashMap<String, String>,
    /// Number of keys that had metadata (used for diagnostics).
    pub meta_found: usize,
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

        let size = bytes.len() as u64;

        // Content-hash dedup: compute SHA-256 and derive blob key.
        let content_hash =
            Sha256::digest(bytes)
                .iter()
                .fold(String::with_capacity(64), |mut s, b| {
                    use std::fmt::Write;
                    let _ = write!(s, "{:02x}", b);
                    s
                });
        let blob_key = content_blob_key(namespace_id, &content_hash);

        // Fetch existing meta to find old blob key for cleanup.
        let old_blob_key = self
            .object_meta_store
            .get_object_meta(namespace_id, key)
            .await?
            .and_then(|m| m.blob_key);

        // Skip R2 write if an identical blob already exists.
        let blob_exists = self.blob_store.exists(&blob_key).await?;
        if !blob_exists {
            // Build R2 metadata with audience info. The audience's current
            // gate set is serialized into a `gates` JSON string so readers
            // querying R2 directly can reason about access without a DB hit.
            let r2_metadata = match audience {
                Some(aud) => {
                    let mut m = HashMap::new();
                    m.insert("audience".to_string(), aud.to_string());
                    if let Some(info) = self.namespace_store.get_audience(namespace_id, aud).await?
                    {
                        let gates_json =
                            serde_json::to_string(&info.gates).unwrap_or_else(|_| "[]".to_string());
                        m.insert("gates".to_string(), gates_json);
                    }
                    Some(m)
                }
                None => None,
            };

            self.blob_store
                .put(&blob_key, bytes, mime_type, r2_metadata.as_ref())
                .await?;
        }

        self.object_meta_store
            .upsert_object(
                namespace_id,
                key,
                &blob_key,
                mime_type,
                size,
                audience,
                Some(&content_hash),
            )
            .await?;

        // Clean up old blob if it differs and is no longer referenced.
        if let Some(ref old_key) = old_blob_key {
            if *old_key != blob_key {
                let refs = self
                    .object_meta_store
                    .count_refs_to_blob(namespace_id, old_key)
                    .await?;
                if refs == 0 {
                    self.blob_store.delete(old_key).await?;
                }
            }
        }

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

    /// Fetch multiple objects in a single call. Uses a batch metadata query
    /// and parallel blob fetches for performance.
    pub async fn get_batch(
        &self,
        namespace_id: &str,
        keys: &[String],
        caller_user_id: &str,
    ) -> Result<BatchGetResult, ServerCoreError> {
        const MAX_BATCH_KEYS: usize = 500;

        if keys.len() > MAX_BATCH_KEYS {
            return Err(ServerCoreError::invalid_input(format!(
                "batch request exceeds maximum of {} keys",
                MAX_BATCH_KEYS
            )));
        }

        self.require_namespace_owner(namespace_id, caller_user_id)
            .await?;

        let mut objects = HashMap::new();
        let mut errors = HashMap::new();

        // 1. Batch metadata query — single DB round-trip for all keys.
        let meta_list = self
            .object_meta_store
            .get_objects_meta_batch(namespace_id, keys)
            .await?;

        let meta_map: HashMap<String, &ObjectMeta> =
            meta_list.iter().map(|m| (m.key.clone(), m)).collect();

        // Track which keys had no metadata.
        let mut blob_requests: Vec<(String, String, String)> = Vec::new(); // (key, blob_key, mime_type)
        for key in keys {
            match meta_map.get(key) {
                Some(meta) => {
                    let blob_key = meta
                        .blob_key
                        .clone()
                        .unwrap_or_else(|| object_blob_key(namespace_id, key));
                    blob_requests.push((key.clone(), blob_key, meta.mime_type.clone()));
                }
                None => {
                    errors.insert(key.clone(), "Object not found".to_string());
                }
            }
        }

        // 2. Parallel blob fetches — all R2/disk reads run concurrently.
        let blob_futures: Vec<_> = blob_requests
            .iter()
            .map(|(_, blob_key, _)| self.blob_store.get(blob_key))
            .collect();

        let blob_results: Vec<_> = futures::future::join_all(blob_futures).await;

        let mut total_bytes_out: u64 = 0;
        for (i, result) in blob_results.into_iter().enumerate() {
            let (key, _, mime_type) = &blob_requests[i];
            match result {
                Ok(Some(bytes)) => {
                    total_bytes_out += bytes.len() as u64;
                    objects.insert(
                        key.clone(),
                        GetObjectResult {
                            mime_type: mime_type.clone(),
                            bytes,
                        },
                    );
                }
                Ok(None) => {
                    errors.insert(key.clone(), "Object not found".to_string());
                }
                Err(e) => {
                    errors.insert(key.clone(), e.to_string());
                }
            }
        }

        if total_bytes_out > 0 {
            let _ = self
                .object_meta_store
                .record_usage(
                    caller_user_id,
                    "bytes_out",
                    total_bytes_out,
                    Some(namespace_id),
                )
                .await;
        }

        Ok(BatchGetResult {
            objects,
            errors,
            meta_found: blob_requests.len(),
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

        // Delete metadata first (drops ref count), then conditionally delete blob.
        self.object_meta_store
            .delete_object(namespace_id, key)
            .await?;

        let refs = self
            .object_meta_store
            .count_refs_to_blob(namespace_id, &blob_key)
            .await?;
        if refs == 0 {
            self.blob_store.delete(&blob_key).await?;
        }

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
            gates: audience.gates,
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

/// Derive the blob store key for a namespace object (legacy layout).
pub fn object_blob_key(namespace_id: &str, key: &str) -> String {
    format!("ns/{}/{}", namespace_id, key)
}

/// Derive the content-addressed blob key for a namespace object.
fn content_blob_key(namespace_id: &str, content_hash: &str) -> String {
    format!("ns/{}/blobs/{}", namespace_id, content_hash)
}

#[cfg(test)]
mod tests {
    use super::ObjectService;
    use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo, ObjectMeta, UsageTotals};
    use crate::ports::{
        BlobStore, MultipartCompletedPart, NamespaceStore, ObjectMetaStore, ServerCoreError,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestNamespaceStore {
        namespaces: Mutex<HashMap<String, NamespaceInfo>>,
        audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
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
    struct TestObjectMetaStore {
        objects: Mutex<HashMap<(String, String), ObjectMeta>>,
        usage: Mutex<Vec<(String, String, u64)>>,
    }

    crate::cfg_async_trait! {
    impl ObjectMetaStore for TestObjectMetaStore {
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
                    content_hash: content_hash.map(|s| s.to_string()),
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
        async fn get_objects_meta_batch(
            &self,
            namespace_id: &str,
            keys: &[String],
        ) -> Result<Vec<ObjectMeta>, ServerCoreError> {
            let store = self.objects.lock().unwrap();
            Ok(keys
                .iter()
                .filter_map(|key| {
                    store
                        .get(&(namespace_id.to_string(), key.clone()))
                        .cloned()
                })
                .collect())
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
        async fn count_refs_to_blob(
            &self,
            namespace_id: &str,
            blob_key: &str,
        ) -> Result<u64, ServerCoreError> {
            let count = self
                .objects
                .lock()
                .unwrap()
                .values()
                .filter(|o| {
                    o.namespace_id == namespace_id
                        && o.blob_key.as_deref() == Some(blob_key)
                })
                .count();
            Ok(count as u64)
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
        async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
            Ok(self.blobs.lock().unwrap().get(key).cloned())
        }
        async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
            self.blobs.lock().unwrap().remove(key);
            Ok(())
        }
        async fn exists(&self, key: &str) -> Result<bool, ServerCoreError> {
            Ok(self.blobs.lock().unwrap().contains_key(key))
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

    fn make_stores() -> (TestNamespaceStore, TestObjectMetaStore, TestBlobStore) {
        let ns_store = TestNamespaceStore::default();
        ns_store.namespaces.lock().unwrap().insert(
            "ns1".to_string(),
            NamespaceInfo {
                id: "ns1".to_string(),
                owner_user_id: "user1".to_string(),
                created_at: 1,
                metadata: None,
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
                gates: vec![],
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
        // Blob is content-addressed; should be cleaned up when no refs remain.
        assert!(blob_store.blobs.lock().unwrap().is_empty());
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
                gates: vec![],
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
        assert!(access.gates.is_empty());
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

    #[tokio::test]
    async fn dedup_same_bytes_under_two_keys() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put("ns1", "a.txt", "text/plain", b"same content", None, "user1")
            .await
            .unwrap();
        service
            .put("ns1", "b.txt", "text/plain", b"same content", None, "user1")
            .await
            .unwrap();

        // Both keys exist in meta store.
        assert!(
            obj_store
                .objects
                .lock()
                .unwrap()
                .contains_key(&("ns1".into(), "a.txt".into()))
        );
        assert!(
            obj_store
                .objects
                .lock()
                .unwrap()
                .contains_key(&("ns1".into(), "b.txt".into()))
        );

        // Only one blob in blob store (deduped).
        assert_eq!(blob_store.blobs.lock().unwrap().len(), 1);

        // Both meta rows point to the same blob key.
        let a_blob = obj_store
            .objects
            .lock()
            .unwrap()
            .get(&("ns1".into(), "a.txt".into()))
            .unwrap()
            .blob_key
            .clone();
        let b_blob = obj_store
            .objects
            .lock()
            .unwrap()
            .get(&("ns1".into(), "b.txt".into()))
            .unwrap()
            .blob_key
            .clone();
        assert_eq!(a_blob, b_blob);
    }

    #[tokio::test]
    async fn ref_counted_delete() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put("ns1", "a.txt", "text/plain", b"shared", None, "user1")
            .await
            .unwrap();
        service
            .put("ns1", "b.txt", "text/plain", b"shared", None, "user1")
            .await
            .unwrap();

        // Delete one key — blob should survive.
        service.delete("ns1", "a.txt", "user1").await.unwrap();
        assert_eq!(blob_store.blobs.lock().unwrap().len(), 1);

        // Delete the other — blob should be removed.
        service.delete("ns1", "b.txt", "user1").await.unwrap();
        assert!(blob_store.blobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn overwrite_cleans_up_old_blob() {
        let (ns_store, obj_store, blob_store) = make_stores();
        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        service
            .put("ns1", "file.txt", "text/plain", b"content A", None, "user1")
            .await
            .unwrap();
        assert_eq!(blob_store.blobs.lock().unwrap().len(), 1);

        service
            .put("ns1", "file.txt", "text/plain", b"content B", None, "user1")
            .await
            .unwrap();

        // Old blob for "content A" should be cleaned up; only "content B" blob remains.
        assert_eq!(blob_store.blobs.lock().unwrap().len(), 1);
        let meta = obj_store
            .objects
            .lock()
            .unwrap()
            .get(&("ns1".into(), "file.txt".into()))
            .unwrap()
            .clone();
        assert!(meta.content_hash.is_some());
        assert!(meta.blob_key.as_ref().unwrap().contains("blobs/"));
    }

    #[tokio::test]
    async fn legacy_rows_without_content_hash_still_work() {
        let (ns_store, obj_store, blob_store) = make_stores();

        // Simulate a legacy row: blob_key uses old format, no content_hash.
        let legacy_blob_key = "ns/ns1/legacy.txt";
        blob_store
            .blobs
            .lock()
            .unwrap()
            .insert(legacy_blob_key.to_string(), b"legacy data".to_vec());
        obj_store.objects.lock().unwrap().insert(
            ("ns1".into(), "legacy.txt".into()),
            ObjectMeta {
                namespace_id: "ns1".into(),
                key: "legacy.txt".into(),
                blob_key: Some(legacy_blob_key.into()),
                mime_type: "text/plain".into(),
                size_bytes: 11,
                updated_at: 1,
                audience: None,
                content_hash: None,
            },
        );

        let service = ObjectService::new(&ns_store, &obj_store, &blob_store);

        // get() should work with legacy blob key.
        let obj = service.get("ns1", "legacy.txt", "user1").await.unwrap();
        assert_eq!(obj.bytes, b"legacy data");

        // delete() should clean up legacy blob.
        service.delete("ns1", "legacy.txt", "user1").await.unwrap();
        assert!(blob_store.blobs.lock().unwrap().is_empty());
    }
}
