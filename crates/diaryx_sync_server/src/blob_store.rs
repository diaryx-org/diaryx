use crate::config::R2Config;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{RequestChecksumCalculation, ResponseChecksumValidation};
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_smithy_types::byte_stream::ByteStream;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[async_trait]
pub trait BlobStore: Send + Sync {
    fn blob_key(&self, user_id: &str, hash: &str) -> String;

    async fn put(&self, key: &str, bytes: &[u8], mime_type: &str) -> Result<(), String>;

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String>;

    async fn delete(&self, key: &str) -> Result<(), String>;

    async fn exists(&self, key: &str) -> Result<bool, String>;
}

#[derive(Clone)]
pub struct R2BlobStore {
    client: Client,
    bucket: String,
    prefix: String,
}

impl R2BlobStore {
    pub async fn new(config: &R2Config) -> Result<Self, String> {
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

    async fn put(&self, key: &str, bytes: &[u8], mime_type: &str) -> Result<(), String> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(mime_type)
            .content_length(bytes.len() as i64)
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|e| {
                let code = e.code().unwrap_or("unknown");
                let message = e.message().unwrap_or("unknown");
                format!(
                    "R2 put failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket, key, code, message, e
                )
            })?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        match response {
            Ok(out) => {
                let body = out
                    .body
                    .collect()
                    .await
                    .map_err(|e| format!("R2 get body failed for {}: {}", key, e))?;
                Ok(Some(body.into_bytes().to_vec()))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NoSuchKey") || msg.contains("404") || msg.contains("Not Found") {
                    Ok(None)
                } else {
                    let code = e.code().unwrap_or("unknown");
                    let message = e.message().unwrap_or("unknown");
                    Err(format!(
                        "R2 get failed for bucket={} key={}: code={} message={} raw={:?}",
                        self.bucket, key, code, message, e
                    ))
                }
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<(), String> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let code = e.code().unwrap_or("unknown");
                let message = e.message().unwrap_or("unknown");
                format!(
                    "R2 delete failed for bucket={} key={}: code={} message={} raw={:?}",
                    self.bucket, key, code, message, e
                )
            })?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, String> {
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
                if msg.contains("NoSuchKey") || msg.contains("404") || msg.contains("Not Found") {
                    Ok(false)
                } else {
                    let code = e.code().unwrap_or("unknown");
                    let message = e.message().unwrap_or("unknown");
                    Err(format!(
                        "R2 exists failed for bucket={} key={}: code={} message={} raw={:?}",
                        self.bucket, key, code, message, e
                    ))
                }
            }
        }
    }
}

#[derive(Default)]
pub struct InMemoryBlobStore {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    prefix: String,
}

impl InMemoryBlobStore {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
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

    async fn put(&self, key: &str, bytes: &[u8], _mime_type: &str) -> Result<(), String> {
        self.blobs
            .lock()
            .map_err(|_| "Failed to lock in-memory blob store".to_string())?
            .insert(key.to_string(), bytes.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self
            .blobs
            .lock()
            .map_err(|_| "Failed to lock in-memory blob store".to_string())?
            .get(key)
            .cloned())
    }

    async fn delete(&self, key: &str) -> Result<(), String> {
        self.blobs
            .lock()
            .map_err(|_| "Failed to lock in-memory blob store".to_string())?
            .remove(key);
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, String> {
        Ok(self
            .blobs
            .lock()
            .map_err(|_| "Failed to lock in-memory blob store".to_string())?
            .contains_key(key))
    }
}

pub async fn build_blob_store(
    config: &crate::config::Config,
) -> Result<Arc<dyn BlobStore>, String> {
    if config.is_r2_configured() {
        let store = R2BlobStore::new(&config.r2).await?;
        Ok(Arc::new(store))
    } else {
        Ok(Arc::new(InMemoryBlobStore::new(config.r2.prefix.clone())))
    }
}
