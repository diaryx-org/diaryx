use crate::config::R2Config;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{RequestChecksumCalculation, ResponseChecksumValidation};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart, Delete, ObjectIdentifier};
use aws_smithy_types::byte_stream::ByteStream;
use diaryx_server::ports::ServerCoreError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use diaryx_server::ports::{BlobStore, MultipartCompletedPart};

fn internal_error(message: impl Into<String>) -> ServerCoreError {
    ServerCoreError::internal(message.into())
}

#[derive(Clone)]
pub struct R2BlobStore {
    client: Client,
    bucket: String,
    prefix: String,
}

impl R2BlobStore {
    pub async fn new(config: &R2Config) -> Result<Self, ServerCoreError> {
        let endpoint = config
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("https://{}.r2.cloudflarestorage.com", config.account_id));

        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("auto"))
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                &config.access_key_id,
                &config.secret_access_key,
                None,
                None,
                "diaryx-r2",
            ))
            .endpoint_url(endpoint)
            .load()
            .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
            .force_path_style(true)
            // R2 can reject optional checksum behavior used by newer S3 SDK defaults.
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
            .build();

        Ok(Self {
            client: Client::from_conf(s3_config),
            bucket: config.bucket.clone(),
            prefix: config.prefix.trim_matches('/').to_string(),
        })
    }
}

#[async_trait]
impl BlobStore for R2BlobStore {
    fn blob_key(&self, user_id: &str, hash: &str) -> String {
        if self.prefix.is_empty() {
            format!("u/{}/blobs/{}", user_id, hash)
        } else {
            format!("{}/u/{}/blobs/{}", self.prefix, user_id, hash)
        }
    }

    fn prefix(&self) -> &str {
        &self.prefix
    }

    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        metadata: Option<&HashMap<String, String>>,
    ) -> Result<(), ServerCoreError> {
        let mut req = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(mime_type)
            .content_length(bytes.len() as i64)
            .body(ByteStream::from(bytes.to_vec()));

        if let Some(meta) = metadata {
            for (k, v) in meta {
                req = req.metadata(k, v);
            }
        }

        req.send().await.map_err(|e| {
            internal_error(format!(
                "R2 put failed for bucket={} key={}: code={} message={} raw={:?}",
                self.bucket,
                key,
                e.code().unwrap_or("unknown"),
                e.message().unwrap_or("unknown"),
                e
            ))
        })?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match response {
            Ok(out) => {
                let body = out.body.collect().await.map_err(|e| {
                    internal_error(format!("R2 get body failed for {}: {}", key, e))
                })?;
                Ok(Some(body.into_bytes().to_vec()))
            }
            Err(e) => {
                let msg = e.to_string();
                let code = e.code().unwrap_or("");
                if code == "NoSuchKey"
                    || msg.contains("NoSuchKey")
                    || msg.contains("404")
                    || msg.contains("Not Found")
                {
                    Ok(None)
                } else {
                    Err(internal_error(format!(
                        "R2 get failed for bucket={} key={}: code={} message={} raw={:?}",
                        self.bucket,
                        key,
                        e.code().unwrap_or("unknown"),
                        e.message().unwrap_or("unknown"),
                        e
                    )))
                }
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                internal_error(format!(
                    "R2 delete failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket,
                    key,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, ServerCoreError> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                let code = e.code().unwrap_or("");
                if code == "NoSuchKey"
                    || msg.contains("NoSuchKey")
                    || msg.contains("404")
                    || msg.contains("Not Found")
                {
                    Ok(false)
                } else {
                    Err(internal_error(format!(
                        "R2 exists failed for bucket={} key={}: code={} message={} raw={:?}",
                        self.bucket,
                        key,
                        e.code().unwrap_or("unknown"),
                        e.message().unwrap_or("unknown"),
                        e
                    )))
                }
            }
        }
    }

    async fn init_multipart(&self, key: &str, mime_type: &str) -> Result<String, ServerCoreError> {
        let response = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .content_type(mime_type)
            .send()
            .await
            .map_err(|e| {
                internal_error(format!(
                    "R2 multipart init failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket,
                    key,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;

        response
            .upload_id
            .ok_or_else(|| internal_error("R2 multipart init missing upload_id"))
    }

    async fn upload_part(
        &self,
        key: &str,
        multipart_id: &str,
        part_no: u32,
        bytes: &[u8],
    ) -> Result<String, ServerCoreError> {
        let response = self
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(multipart_id)
            .part_number(part_no as i32)
            .content_length(bytes.len() as i64)
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|e| {
                internal_error(format!(
                    "R2 multipart part upload failed for bucket={} key={} part={}: code={} message={} raw={:?}",
                    self.bucket,
                    key,
                    part_no,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;

        response.e_tag.ok_or_else(|| {
            internal_error(format!("R2 multipart upload part {} missing etag", part_no))
        })
    }

    async fn complete_multipart(
        &self,
        key: &str,
        multipart_id: &str,
        parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError> {
        let completed_parts = parts
            .iter()
            .map(|part| {
                CompletedPart::builder()
                    .set_part_number(Some(part.part_no as i32))
                    .set_e_tag(Some(part.etag.clone()))
                    .build()
            })
            .collect::<Vec<_>>();
        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(multipart_id)
            .multipart_upload(completed_upload)
            .send()
            .await
            .map_err(|e| {
                internal_error(format!(
                    "R2 multipart complete failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket,
                    key,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;
        Ok(())
    }

    async fn abort_multipart(&self, key: &str, multipart_id: &str) -> Result<(), ServerCoreError> {
        self.client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(multipart_id)
            .send()
            .await
            .map_err(|e| {
                internal_error(format!(
                    "R2 multipart abort failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket,
                    key,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;
        Ok(())
    }

    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .range(format!("bytes={}-{}", range_start, range_end))
            .send()
            .await;

        match response {
            Ok(out) => {
                let body = out.body.collect().await.map_err(|e| {
                    internal_error(format!("R2 get range body failed for {}: {}", key, e))
                })?;
                Ok(Some(body.into_bytes().to_vec()))
            }
            Err(e) => {
                let msg = e.to_string();
                let code = e.code().unwrap_or("");
                if code == "NoSuchKey"
                    || msg.contains("NoSuchKey")
                    || msg.contains("404")
                    || msg.contains("Not Found")
                {
                    Ok(None)
                } else {
                    Err(internal_error(format!(
                        "R2 get range failed for bucket={} key={}: code={} message={} raw={:?}",
                        self.bucket,
                        key,
                        e.code().unwrap_or("unknown"),
                        e.message().unwrap_or("unknown"),
                        e
                    )))
                }
            }
        }
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError> {
        let mut keys = Vec::new();
        let mut continuation: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(token) = &continuation {
                req = req.continuation_token(token);
            }

            let response = req.send().await.map_err(|e| {
                internal_error(format!(
                    "R2 list failed for bucket={} prefix={}: code={} message={} raw={:?}",
                    self.bucket,
                    prefix,
                    e.code().unwrap_or("unknown"),
                    e.message().unwrap_or("unknown"),
                    e
                ))
            })?;

            for object in response.contents() {
                if let Some(key) = object.key() {
                    keys.push(key.to_string());
                }
            }

            if response.is_truncated().unwrap_or(false) {
                continuation = response.next_continuation_token().map(str::to_string);
                if continuation.is_none() {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(keys)
    }

    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError> {
        let keys = self.list_by_prefix(prefix).await?;
        if keys.is_empty() {
            return Ok(0);
        }

        let mut deleted = 0usize;
        for chunk in keys.chunks(1000) {
            let mut objects = Vec::with_capacity(chunk.len());
            for key in chunk {
                let object = ObjectIdentifier::builder().key(key).build().map_err(|e| {
                    internal_error(format!("R2 delete object identifier build failed: {}", e))
                })?;
                objects.push(object);
            }

            let delete = Delete::builder()
                .set_objects(Some(objects))
                .build()
                .map_err(|e| internal_error(format!("R2 delete request build failed: {}", e)))?;

            self.client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(delete)
                .send()
                .await
                .map_err(|e| {
                    internal_error(format!(
                        "R2 delete-by-prefix failed for bucket={} prefix={}: code={} message={} raw={:?}",
                        self.bucket,
                        prefix,
                        e.code().unwrap_or("unknown"),
                        e.message().unwrap_or("unknown"),
                        e
                    ))
                })?;
            deleted += chunk.len();
        }

        Ok(deleted)
    }
}

#[derive(Default)]
pub struct InMemoryBlobStore {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    multipart: Mutex<HashMap<String, HashMap<u32, Vec<u8>>>>,
    prefix: String,
}

impl InMemoryBlobStore {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
            multipart: Mutex::new(HashMap::new()),
            prefix: prefix.into(),
        }
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    fn blob_key(&self, user_id: &str, hash: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        if prefix.is_empty() {
            format!("u/{}/blobs/{}", user_id, hash)
        } else {
            format!("{}/u/{}/blobs/{}", prefix, user_id, hash)
        }
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
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?
            .insert(key.to_string(), bytes.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
        Ok(self
            .blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?
            .get(key)
            .cloned())
    }

    async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
        self.blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?
            .remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, ServerCoreError> {
        Ok(self
            .blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?
            .contains_key(key))
    }

    async fn init_multipart(
        &self,
        _key: &str,
        _mime_type: &str,
    ) -> Result<String, ServerCoreError> {
        let upload_id = uuid::Uuid::new_v4().to_string();
        self.multipart
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory multipart store"))?
            .insert(upload_id.clone(), HashMap::new());
        Ok(upload_id)
    }

    async fn upload_part(
        &self,
        _key: &str,
        multipart_id: &str,
        part_no: u32,
        bytes: &[u8],
    ) -> Result<String, ServerCoreError> {
        let mut sessions = self
            .multipart
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory multipart store"))?;
        let parts = sessions.get_mut(multipart_id).ok_or_else(|| {
            internal_error(format!("Unknown multipart session: {}", multipart_id))
        })?;
        parts.insert(part_no, bytes.to_vec());
        Ok(format!("inmem-etag-{}-{}", multipart_id, part_no))
    }

    async fn complete_multipart(
        &self,
        key: &str,
        multipart_id: &str,
        parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError> {
        let mut sessions = self
            .multipart
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory multipart store"))?;
        let stored = sessions.remove(multipart_id).ok_or_else(|| {
            internal_error(format!("Unknown multipart session: {}", multipart_id))
        })?;
        drop(sessions);

        let mut ordered = parts.to_vec();
        ordered.sort_by_key(|p| p.part_no);
        let mut bytes = Vec::new();
        for part in ordered {
            let chunk = stored
                .get(&part.part_no)
                .ok_or_else(|| internal_error(format!("Missing part {}", part.part_no)))?;
            bytes.extend_from_slice(chunk);
        }

        self.blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?
            .insert(key.to_string(), bytes);
        Ok(())
    }

    async fn abort_multipart(&self, _key: &str, multipart_id: &str) -> Result<(), ServerCoreError> {
        self.multipart
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory multipart store"))?
            .remove(multipart_id);
        Ok(())
    }

    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let blobs = self
            .blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?;
        let Some(bytes) = blobs.get(key) else {
            return Ok(None);
        };

        if bytes.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let start = (range_start.min(bytes.len() as u64 - 1)) as usize;
        let end = (range_end.min(bytes.len() as u64 - 1)) as usize;
        if start > end {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(bytes[start..=end].to_vec()))
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError> {
        let blobs = self
            .blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?;
        Ok(blobs
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError> {
        let mut blobs = self
            .blobs
            .lock()
            .map_err(|_| internal_error("Failed to lock in-memory blob store"))?;
        let keys: Vec<String> = blobs
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        let deleted = keys.len();
        for key in keys {
            blobs.remove(&key);
        }
        Ok(deleted)
    }
}

/// Filesystem-backed blob store that persists blobs to a local directory.
pub struct LocalFsBlobStore {
    root: std::path::PathBuf,
    multipart_dir: std::path::PathBuf,
    prefix: String,
}

impl LocalFsBlobStore {
    pub fn new(
        root: impl Into<std::path::PathBuf>,
        prefix: impl Into<String>,
    ) -> Result<Self, ServerCoreError> {
        let root = root.into();
        let multipart_dir = root.join(".multipart");
        std::fs::create_dir_all(&root)
            .map_err(|e| internal_error(format!("Failed to create blob dir {:?}: {}", root, e)))?;
        std::fs::create_dir_all(&multipart_dir).map_err(|e| {
            internal_error(format!(
                "Failed to create multipart dir {:?}: {}",
                multipart_dir, e
            ))
        })?;
        Ok(Self {
            root,
            multipart_dir,
            prefix: prefix.into(),
        })
    }

    fn key_to_path(&self, key: &str) -> std::path::PathBuf {
        // Sanitize key: replace potentially problematic chars, keep `/` as dir separator
        self.root.join(key)
    }

    fn multipart_session_dir(&self, multipart_id: &str) -> std::path::PathBuf {
        self.multipart_dir.join(multipart_id)
    }
}

#[async_trait]
impl BlobStore for LocalFsBlobStore {
    fn blob_key(&self, user_id: &str, hash: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        if prefix.is_empty() {
            format!("u/{}/blobs/{}", user_id, hash)
        } else {
            format!("{}/u/{}/blobs/{}", prefix, user_id, hash)
        }
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
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                internal_error(format!("Failed to create dirs for {:?}: {}", path, e))
            })?;
        }
        std::fs::write(&path, bytes)
            .map_err(|e| internal_error(format!("Failed to write blob {:?}: {}", path, e)))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let path = self.key_to_path(key);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(internal_error(format!(
                "Failed to read blob {:?}: {}",
                path, e
            ))),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
        let path = self.key_to_path(key);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(internal_error(format!(
                "Failed to delete blob {:?}: {}",
                path, e
            ))),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, ServerCoreError> {
        Ok(self.key_to_path(key).exists())
    }

    async fn init_multipart(
        &self,
        _key: &str,
        _mime_type: &str,
    ) -> Result<String, ServerCoreError> {
        let upload_id = uuid::Uuid::new_v4().to_string();
        let session_dir = self.multipart_session_dir(&upload_id);
        std::fs::create_dir_all(&session_dir).map_err(|e| {
            internal_error(format!(
                "Failed to create multipart session dir {:?}: {}",
                session_dir, e
            ))
        })?;
        Ok(upload_id)
    }

    async fn upload_part(
        &self,
        _key: &str,
        multipart_id: &str,
        part_no: u32,
        bytes: &[u8],
    ) -> Result<String, ServerCoreError> {
        let part_path = self
            .multipart_session_dir(multipart_id)
            .join(format!("{:08}", part_no));
        std::fs::write(&part_path, bytes).map_err(|e| {
            internal_error(format!(
                "Failed to write multipart part {:?}: {}",
                part_path, e
            ))
        })?;
        Ok(format!("local-etag-{}-{}", multipart_id, part_no))
    }

    async fn complete_multipart(
        &self,
        key: &str,
        multipart_id: &str,
        parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError> {
        let session_dir = self.multipart_session_dir(multipart_id);
        let mut ordered = parts.to_vec();
        ordered.sort_by_key(|p| p.part_no);

        let mut assembled = Vec::new();
        for part in &ordered {
            let part_path = session_dir.join(format!("{:08}", part.part_no));
            let chunk = std::fs::read(&part_path).map_err(|e| {
                internal_error(format!(
                    "Failed to read multipart part {:?}: {}",
                    part_path, e
                ))
            })?;
            assembled.extend_from_slice(&chunk);
        }

        // Write the final blob
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                internal_error(format!("Failed to create dirs for {:?}: {}", path, e))
            })?;
        }
        std::fs::write(&path, &assembled).map_err(|e| {
            internal_error(format!("Failed to write assembled blob {:?}: {}", path, e))
        })?;

        // Clean up session dir
        let _ = std::fs::remove_dir_all(&session_dir);
        Ok(())
    }

    async fn abort_multipart(&self, _key: &str, multipart_id: &str) -> Result<(), ServerCoreError> {
        let session_dir = self.multipart_session_dir(multipart_id);
        let _ = std::fs::remove_dir_all(&session_dir);
        Ok(())
    }

    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        let path = self.key_to_path(key);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(internal_error(format!(
                    "Failed to read blob {:?}: {}",
                    path, e
                )));
            }
        };
        if bytes.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let start = (range_start.min(bytes.len() as u64 - 1)) as usize;
        let end = (range_end.min(bytes.len() as u64 - 1)) as usize;
        if start > end {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(bytes[start..=end].to_vec()))
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError> {
        let search_path = self.root.join(prefix);
        // Walk the directory that the prefix points into
        let search_dir = if search_path.is_dir() {
            search_path
        } else {
            match search_path.parent() {
                Some(p) if p.exists() => p.to_path_buf(),
                _ => return Ok(Vec::new()),
            }
        };

        let mut keys = Vec::new();
        for entry in walkdir::WalkDir::new(&search_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Ok(rel) = entry.path().strip_prefix(&self.root) {
                    let key = rel.to_string_lossy().to_string();
                    if key.starts_with(prefix) {
                        keys.push(key);
                    }
                }
            }
        }
        Ok(keys)
    }

    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError> {
        let keys = self.list_by_prefix(prefix).await?;
        let deleted = keys.len();
        for key in &keys {
            let path = self.key_to_path(key);
            let _ = std::fs::remove_file(&path);
        }
        Ok(deleted)
    }
}

pub async fn build_blob_store(
    config: &crate::config::Config,
) -> Result<Arc<dyn BlobStore>, ServerCoreError> {
    if config.is_r2_configured() {
        let store = R2BlobStore::new(&config.r2).await?;
        Ok(Arc::new(store))
    } else if config.blob_store_in_memory {
        Ok(Arc::new(InMemoryBlobStore::new(config.r2.prefix.clone())))
    } else {
        let store = LocalFsBlobStore::new(&config.blob_store_path, config.r2.prefix.clone())?;
        Ok(Arc::new(store))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_delete_by_prefix_only_removes_matching_keys() {
        let store = InMemoryBlobStore::new("diaryx-sync");
        store
            .put("site/a/index.html", b"a", "text/html", None)
            .await
            .unwrap();
        store
            .put("site/a/page.html", b"b", "text/html", None)
            .await
            .unwrap();
        store
            .put("site/b/index.html", b"c", "text/html", None)
            .await
            .unwrap();

        let deleted = store.delete_by_prefix("site/a/").await.unwrap();
        assert_eq!(deleted, 2);
        assert!(store.get("site/a/index.html").await.unwrap().is_none());
        assert!(store.get("site/a/page.html").await.unwrap().is_none());
        assert!(store.get("site/b/index.html").await.unwrap().is_some());
    }
}
