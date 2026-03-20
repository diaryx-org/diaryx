//! KV adapter for the DomainMappingCache trait.
//!
//! On the native server, domain mappings are synced to KV via the
//! Cloudflare HTTP API. Here we bind directly to the KV namespace.

use async_trait::async_trait;
use diaryx_server::ports::{DomainMappingCache, ServerCoreError};
use worker::kv::KvStore;

fn e(err: impl std::fmt::Display) -> ServerCoreError {
    ServerCoreError::internal(err.to_string())
}

pub struct KvDomainMappingCache {
    kv: KvStore,
}

impl KvDomainMappingCache {
    pub fn new(kv: KvStore) -> Self {
        Self { kv }
    }
}

#[async_trait(?Send)]
impl DomainMappingCache for KvDomainMappingCache {
    async fn put_domain(
        &self,
        hostname: &str,
        namespace_id: &str,
        audience_name: &str,
    ) -> Result<(), ServerCoreError> {
        let value = serde_json::json!({
            "namespace_id": namespace_id,
            "audience_name": audience_name,
        });
        self.kv
            .put(&format!("domain:{hostname}"), value.to_string())
            .map_err(e)?
            .execute()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn delete_domain(&self, hostname: &str) -> Result<(), ServerCoreError> {
        self.kv
            .delete(&format!("domain:{hostname}"))
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn put_subdomain(
        &self,
        subdomain: &str,
        namespace_id: &str,
        default_audience: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let mut value = serde_json::json!({ "namespace_id": namespace_id });
        if let Some(aud) = default_audience {
            value["default_audience"] = serde_json::Value::String(aud.to_string());
        }
        self.kv
            .put(
                &format!("subdomain:{}", subdomain.to_lowercase()),
                value.to_string(),
            )
            .map_err(e)?
            .execute()
            .await
            .map_err(e)?;
        Ok(())
    }

    async fn delete_subdomain(&self, subdomain: &str) -> Result<(), ServerCoreError> {
        self.kv
            .delete(&format!("subdomain:{}", subdomain.to_lowercase()))
            .await
            .map_err(e)?;
        Ok(())
    }
}
