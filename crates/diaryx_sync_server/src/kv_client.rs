use tracing::warn;

/// Cloudflare KV REST API client for domain→slug mappings.
pub struct CloudflareKvClient {
    base_url: String,
    api_token: String,
    client: reqwest::Client,
}

impl CloudflareKvClient {
    pub fn new(account_id: String, namespace_id: String, api_token: String) -> Self {
        let base_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/storage/kv/namespaces/{}/values",
            account_id, namespace_id
        );
        Self {
            base_url,
            api_token,
            client: reqwest::Client::new(),
        }
    }

    /// Write a domain→slug mapping: key `domain:{hostname}` with value `{slug}`.
    pub async fn put_domain_mapping(&self, hostname: &str, slug: &str) -> Result<(), String> {
        let url = format!("{}/domain:{}", self.base_url, hostname);
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.api_token)
            .header("Content-Type", "text/plain")
            .body(slug.to_string())
            .send()
            .await
            .map_err(|e| format!("KV PUT request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!("KV PUT domain:{} failed: {} {}", hostname, status, body);
            return Err(format!("KV PUT failed: {} {}", status, body));
        }
        Ok(())
    }

    /// Delete a domain mapping by key `domain:{hostname}`.
    pub async fn delete_domain_mapping(&self, hostname: &str) -> Result<(), String> {
        let url = format!("{}/domain:{}", self.base_url, hostname);
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map_err(|e| format!("KV DELETE request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!("KV DELETE domain:{} failed: {} {}", hostname, status, body);
            return Err(format!("KV DELETE failed: {} {}", status, body));
        }
        Ok(())
    }
}
