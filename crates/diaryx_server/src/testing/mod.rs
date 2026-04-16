//! In-memory, thread-safe implementations of the [`ports`](crate::ports) traits
//! for testing use cases without a real database or blob store.
//!
//! These stores are intentionally minimal: they cover the subset of behavior
//! exercised by the [`use_cases`](crate::use_cases) services, and intentionally
//! leave out server-adapter concerns (migrations, connection pooling, retries).
//! They are also the default backing for the [`contract`](crate::contract)
//! test suite.
//!
//! ## Scope
//!
//! - Supported: namespace + audience + object CRUD, blob put/get/exists/delete,
//!   usage recording and totals.
//! - Not yet supported: multipart uploads, range reads, listing by prefix,
//!   custom domains. These `todo!()` rather than returning a stub, so tests
//!   that depend on them fail loudly rather than silently passing.
//!
//! Extend as needed — but keep behavior aligned with what the real adapters
//! do, or contract tests will drift.
//!
//! ## Example
//!
//! ```no_run
//! # use diaryx_server::testing::{InMemoryNamespaceStore, InMemoryObjectMetaStore, InMemoryBlobStore};
//! # use diaryx_server::use_cases::objects::ObjectService;
//! # async fn example() {
//! let ns = InMemoryNamespaceStore::new();
//! let meta = InMemoryObjectMetaStore::new();
//! let blob = InMemoryBlobStore::new();
//! let svc = ObjectService::new(&ns, &meta, &blob);
//! # let _ = svc;
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::{AudienceInfo, CustomDomainInfo, NamespaceInfo, ObjectMeta, UsageTotals};
use crate::ports::{
    BlobStore, MultipartCompletedPart, NamespaceStore, ObjectMetaStore, ServerCoreError,
};

// ---------------------------------------------------------------------------
// NamespaceStore
// ---------------------------------------------------------------------------

/// Thread-safe, in-memory [`NamespaceStore`] implementation.
#[derive(Default)]
pub struct InMemoryNamespaceStore {
    namespaces: Mutex<HashMap<String, NamespaceInfo>>,
    audiences: Mutex<HashMap<(String, String), AudienceInfo>>,
    custom_domains: Mutex<HashMap<String, CustomDomainInfo>>,
}

impl InMemoryNamespaceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NamespaceStore for InMemoryNamespaceStore {
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
        offset: u32,
    ) -> Result<Vec<NamespaceInfo>, ServerCoreError> {
        let mut matches: Vec<NamespaceInfo> = self
            .namespaces
            .lock()
            .unwrap()
            .values()
            .filter(|ns| ns.owner_user_id == owner_user_id)
            .cloned()
            .collect();
        matches.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(matches
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
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

    async fn get_custom_domain(
        &self,
        domain: &str,
    ) -> Result<Option<CustomDomainInfo>, ServerCoreError> {
        Ok(self.custom_domains.lock().unwrap().get(domain).cloned())
    }

    async fn list_custom_domains(
        &self,
        namespace_id: &str,
    ) -> Result<Vec<CustomDomainInfo>, ServerCoreError> {
        Ok(self
            .custom_domains
            .lock()
            .unwrap()
            .values()
            .filter(|d| d.namespace_id == namespace_id)
            .cloned()
            .collect())
    }

    async fn upsert_custom_domain(
        &self,
        domain: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        let mut map = self.custom_domains.lock().unwrap();
        let entry = map
            .entry(domain.to_string())
            .or_insert_with(|| CustomDomainInfo {
                domain: domain.to_string(),
                namespace_id: namespace_id.to_string(),
                audience_name: audience_name.to_string(),
                created_at: 0,
                verified: false,
            });
        entry.namespace_id = namespace_id.to_string();
        entry.audience_name = audience_name.to_string();
        Ok(())
    }

    async fn delete_custom_domain(&self, domain: &str) -> Result<bool, ServerCoreError> {
        Ok(self.custom_domains.lock().unwrap().remove(domain).is_some())
    }

    async fn create_namespace(
        &self,
        namespace_id: &str,
        owner_user_id: &str,
        metadata: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let mut map = self.namespaces.lock().unwrap();
        if map.contains_key(namespace_id) {
            return Err(ServerCoreError::conflict("namespace already exists"));
        }
        map.insert(
            namespace_id.to_string(),
            NamespaceInfo {
                id: namespace_id.to_string(),
                owner_user_id: owner_user_id.to_string(),
                created_at: 0,
                metadata: metadata.map(str::to_string),
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
        let ns = map
            .get_mut(namespace_id)
            .ok_or_else(|| ServerCoreError::not_found("namespace not found"))?;
        ns.metadata = metadata.map(str::to_string);
        Ok(())
    }

    async fn delete_namespace(&self, namespace_id: &str) -> Result<(), ServerCoreError> {
        self.namespaces.lock().unwrap().remove(namespace_id);
        self.audiences
            .lock()
            .unwrap()
            .retain(|(ns, _), _| ns != namespace_id);
        Ok(())
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
            .iter()
            .filter(|((ns, _), _)| ns == namespace_id)
            .map(|(_, v)| v.clone())
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

    async fn clear_objects_audience(
        &self,
        _namespace_id: &str,
        _audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        // The object meta store owns the audience tag on objects; nothing for
        // the namespace store to do here. Real adapters may enforce cross-store
        // invariants via a transaction — not modeled here.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ObjectMetaStore
// ---------------------------------------------------------------------------

/// Thread-safe, in-memory [`ObjectMetaStore`] implementation.
#[derive(Default)]
pub struct InMemoryObjectMetaStore {
    /// `(namespace_id, key) -> meta`
    objects: Mutex<HashMap<(String, String), ObjectMeta>>,
    /// `user_id -> totals`
    usage: Mutex<HashMap<String, UsageTotals>>,
    /// `(user_id, namespace_id) -> totals`
    namespace_usage: Mutex<HashMap<(String, String), UsageTotals>>,
}

impl InMemoryObjectMetaStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ObjectMetaStore for InMemoryObjectMetaStore {
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
                updated_at: 0,
                audience: audience.map(str::to_string),
                content_hash: content_hash.map(str::to_string),
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
        let map = self.objects.lock().unwrap();
        Ok(keys
            .iter()
            .filter_map(|k| map.get(&(namespace_id.to_string(), k.clone())).cloned())
            .collect())
    }

    async fn list_objects(
        &self,
        namespace_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ObjectMeta>, ServerCoreError> {
        let mut matches: Vec<ObjectMeta> = self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|((ns, _), _)| ns == namespace_id)
            .map(|(_, v)| v.clone())
            .collect();
        matches.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(matches
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    async fn delete_object(&self, namespace_id: &str, key: &str) -> Result<(), ServerCoreError> {
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
        Ok(self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|((ns, _), m)| ns == namespace_id && m.blob_key.as_deref() == Some(blob_key))
            .count() as u64)
    }

    async fn record_usage(
        &self,
        user_id: &str,
        event_type: &str,
        amount: u64,
        namespace_id: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let field = match event_type {
            "bytes_in" => |t: &mut UsageTotals, a: u64| t.bytes_in += a,
            "bytes_out" => |t: &mut UsageTotals, a: u64| t.bytes_out += a,
            "relay_seconds" => |t: &mut UsageTotals, a: u64| t.relay_seconds += a,
            _ => return Ok(()),
        };
        field(
            self.usage
                .lock()
                .unwrap()
                .entry(user_id.to_string())
                .or_default(),
            amount,
        );
        if let Some(ns) = namespace_id {
            field(
                self.namespace_usage
                    .lock()
                    .unwrap()
                    .entry((user_id.to_string(), ns.to_string()))
                    .or_default(),
                amount,
            );
        }
        Ok(())
    }

    async fn get_usage_totals(&self, user_id: &str) -> Result<UsageTotals, ServerCoreError> {
        Ok(self
            .usage
            .lock()
            .unwrap()
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_namespace_usage_totals(
        &self,
        user_id: &str,
        namespace_id: &str,
    ) -> Result<UsageTotals, ServerCoreError> {
        Ok(self
            .namespace_usage
            .lock()
            .unwrap()
            .get(&(user_id.to_string(), namespace_id.to_string()))
            .cloned()
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// BlobStore
// ---------------------------------------------------------------------------

/// Thread-safe, in-memory [`BlobStore`] implementation.
///
/// Multipart and range operations are deliberately unsupported — they
/// `todo!()` so tests that reach for them fail loudly rather than silently
/// relying on stubs.
#[derive(Default)]
pub struct InMemoryBlobStore {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    prefix: String,
}

impl InMemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
            prefix: prefix.into(),
        }
    }

    /// Direct access for test assertions — bypasses the trait.
    pub fn raw_blobs(&self) -> Vec<(String, Vec<u8>)> {
        self.blobs
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    fn blob_key(&self, user_id: &str, hash: &str) -> String {
        format!("{}/users/{}/blobs/{}", self.prefix, user_id, hash)
    }

    fn prefix(&self) -> &str {
        &self.prefix
    }

    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        _mime_type: &str,
        _metadata: Option<&HashMap<String, String>>,
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

    async fn init_multipart(
        &self,
        _key: &str,
        _mime_type: &str,
    ) -> Result<String, ServerCoreError> {
        todo!("InMemoryBlobStore does not support multipart uploads")
    }

    async fn upload_part(
        &self,
        _key: &str,
        _multipart_id: &str,
        _part_no: u32,
        _bytes: &[u8],
    ) -> Result<String, ServerCoreError> {
        todo!("InMemoryBlobStore does not support multipart uploads")
    }

    async fn complete_multipart(
        &self,
        _key: &str,
        _multipart_id: &str,
        _parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError> {
        todo!("InMemoryBlobStore does not support multipart uploads")
    }

    async fn abort_multipart(
        &self,
        _key: &str,
        _multipart_id: &str,
    ) -> Result<(), ServerCoreError> {
        todo!("InMemoryBlobStore does not support multipart uploads")
    }

    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let map = self.blobs.lock().unwrap();
        let Some(bytes) = map.get(key) else {
            return Ok(None);
        };
        let start = range_start as usize;
        let end = (range_end as usize).min(bytes.len());
        if start >= bytes.len() || start >= end {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(bytes[start..end].to_vec()))
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError> {
        let mut keys: Vec<String> = self
            .blobs
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }

    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError> {
        let mut map = self.blobs.lock().unwrap();
        let to_remove: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        for k in &to_remove {
            map.remove(k);
        }
        Ok(to_remove.len())
    }
}
