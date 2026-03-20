//! R2 adapter for the BlobStore trait.

use async_trait::async_trait;
use diaryx_server::ports::{BlobStore, MultipartCompletedPart, ServerCoreError};
use std::collections::HashMap;
use worker::Bucket;

fn e(err: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(err.to_string())
}

pub struct R2BlobStore {
    bucket: Bucket,
}

impl R2BlobStore {
    pub fn new(bucket: Bucket) -> Self {
        Self { bucket }
    }
}

#[async_trait(?Send)]
impl BlobStore for R2BlobStore {
    fn blob_key(&self, user_id: &str, hash: &str) -> String {
        format!("attachments/{}/{}", user_id, hash)
    }

    fn prefix(&self) -> &str {
        "ns"
    }

    async fn put(
        &self,
        key: &str,
        bytes: &[u8],
        mime_type: &str,
        metadata: Option<&HashMap<String, String>>,
    ) -> Result<(), ServerCoreError> {
        let mut builder = self.bucket.put(key, worker::Data::Bytes(bytes.to_vec()));
        builder = builder.http_metadata(worker::HttpMetadata {
            content_type: Some(mime_type.to_string()),
            ..Default::default()
        });
        if let Some(meta) = metadata {
            builder = builder.custom_metadata(meta.clone());
        }
        builder.execute().await.map_err(e)?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ServerCoreError> {
        match self.bucket.get(key).execute().await.map_err(e)? {
            Some(object) => {
                let bytes = object
                    .body()
                    .ok_or_else(|| e("empty body"))?
                    .bytes()
                    .await
                    .map_err(e)?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), ServerCoreError> {
        self.bucket.delete(key).await.map_err(e)?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool, ServerCoreError> {
        Ok(self.bucket.head(key).await.map_err(e)?.is_some())
    }

    async fn init_multipart(&self, key: &str, _mime_type: &str) -> Result<String, ServerCoreError> {
        let upload = self
            .bucket
            .create_multipart_upload(key)
            .execute()
            .await
            .map_err(e)?;
        Ok(upload.upload_id().await)
    }

    async fn upload_part(
        &self,
        _key: &str,
        _multipart_id: &str,
        _part_no: u32,
        _bytes: &[u8],
    ) -> Result<String, ServerCoreError> {
        // R2 multipart via Workers API requires the MultipartUpload handle.
        // Full implementation would cache the handle from init_multipart.
        Err(ServerCoreError::unavailable(
            "Multipart upload not yet implemented for R2 Workers binding",
        ))
    }

    async fn complete_multipart(
        &self,
        _key: &str,
        _multipart_id: &str,
        _parts: &[MultipartCompletedPart],
    ) -> Result<(), ServerCoreError> {
        Err(ServerCoreError::unavailable(
            "Multipart upload not yet implemented for R2 Workers binding",
        ))
    }

    async fn abort_multipart(
        &self,
        _key: &str,
        _multipart_id: &str,
    ) -> Result<(), ServerCoreError> {
        Err(ServerCoreError::unavailable(
            "Multipart upload not yet implemented for R2 Workers binding",
        ))
    }

    async fn get_range(
        &self,
        key: &str,
        range_start: u64,
        range_end: u64,
    ) -> Result<Option<Vec<u8>>, ServerCoreError> {
        match self
            .bucket
            .get(key)
            .range(worker::Range::OffsetWithLength {
                offset: range_start,
                length: range_end - range_start,
            })
            .execute()
            .await
            .map_err(e)?
        {
            Some(object) => {
                let bytes = object
                    .body()
                    .ok_or_else(|| e("empty body"))?
                    .bytes()
                    .await
                    .map_err(e)?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, ServerCoreError> {
        let list = self
            .bucket
            .list()
            .prefix(prefix.to_string())
            .execute()
            .await
            .map_err(e)?;
        Ok(list
            .objects()
            .into_iter()
            .map(|obj| obj.key().to_string())
            .collect())
    }

    async fn delete_by_prefix(&self, prefix: &str) -> Result<usize, ServerCoreError> {
        let keys = self.list_by_prefix(prefix).await?;
        let count = keys.len();
        for key in &keys {
            self.bucket.delete(key).await.map_err(e)?;
        }
        Ok(count)
    }
}
