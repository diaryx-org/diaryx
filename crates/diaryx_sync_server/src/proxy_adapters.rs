//! Native proxy adapter implementations.
//!
//! Provides ProxyConfigStore (static from env), ProxySecretResolver (env + DB),
//! and ProxyUsageStore (SQLite) for the native sync server.

use crate::config::Config;
use crate::db::AuthRepo;
use async_trait::async_trait;
use diaryx_server::proxy::{ProxyAuthMethod, ProxyConfig, ProxyValidation};
use diaryx_server::{
    ProxyConfigStore, ProxySecretResolver, ProxyUsageStore, ServerCoreError, UserTier,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Static proxy config store loaded from server configuration.
pub struct StaticProxyConfigStore {
    proxies: HashMap<String, ProxyConfig>,
}

impl StaticProxyConfigStore {
    pub fn from_config(config: &Config) -> Self {
        let mut proxies = HashMap::new();

        if !config.managed_ai.openrouter_api_key.is_empty() {
            let mut allowed_values = HashMap::new();
            if !config.managed_ai.models.is_empty() {
                allowed_values.insert("model".to_string(), config.managed_ai.models.clone());
            }

            let proxy = ProxyConfig {
                proxy_id: "diaryx.ai".into(),
                upstream: config.managed_ai.openrouter_endpoint.clone(),
                auth_method: ProxyAuthMethod::PlatformSecret {
                    env_key: "MANAGED_AI_OPENROUTER_API_KEY".into(),
                    auth_header: "Authorization".into(),
                    auth_prefix: "Bearer ".into(),
                    required_tier: UserTier::Plus,
                },
                allowed_paths: None,
                rate_limit_per_minute: Some(config.managed_ai.rate_limit_per_minute as u32),
                monthly_quota: Some(config.managed_ai.monthly_quota),
                streaming: true,
                validation: if allowed_values.is_empty() {
                    None
                } else {
                    Some(ProxyValidation {
                        allowed_values,
                        max_body_bytes: None,
                    })
                },
            };
            proxies.insert(proxy.proxy_id.clone(), proxy);
        }

        Self { proxies }
    }
}

#[async_trait]
impl ProxyConfigStore for StaticProxyConfigStore {
    async fn get_proxy(&self, proxy_id: &str) -> Result<Option<ProxyConfig>, ServerCoreError> {
        Ok(self.proxies.get(proxy_id).cloned())
    }

    async fn list_proxies(&self) -> Result<Vec<ProxyConfig>, ServerCoreError> {
        Ok(self.proxies.values().cloned().collect())
    }
}

/// Secret resolver that reads platform secrets from the config and
/// user secrets from the database (placeholder — user secrets not yet implemented).
pub struct NativeProxySecretResolver {
    config: Arc<Config>,
}

impl NativeProxySecretResolver {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

#[async_trait]
impl ProxySecretResolver for NativeProxySecretResolver {
    fn resolve_platform_secret(&self, env_key: &str) -> Option<String> {
        match env_key {
            "MANAGED_AI_OPENROUTER_API_KEY" => {
                let key = &self.config.managed_ai.openrouter_api_key;
                if key.is_empty() {
                    None
                } else {
                    Some(key.clone())
                }
            }
            _ => std::env::var(env_key).ok(),
        }
    }

    async fn resolve_user_secret(&self, _user_id: &str, _secret_key: &str) -> Option<String> {
        // TODO: implement per-user secret store
        None
    }
}

/// Proxy usage store backed by SQLite (reuses user_ai_usage_monthly table).
pub struct NativeProxyUsageStore {
    repo: Arc<AuthRepo>,
}

impl NativeProxyUsageStore {
    pub fn new(repo: Arc<AuthRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ProxyUsageStore for NativeProxyUsageStore {
    async fn get_monthly_count(
        &self,
        user_id: &str,
        _proxy_id: &str,
        period: &str,
    ) -> Result<u64, ServerCoreError> {
        // Currently uses the existing AI usage table (proxy_id not yet in schema)
        self.repo
            .get_user_ai_usage_monthly_count(user_id, period)
            .map_err(|e| ServerCoreError::Internal(e.to_string()))
    }

    async fn increment_monthly_count(
        &self,
        user_id: &str,
        _proxy_id: &str,
        period: &str,
    ) -> Result<u64, ServerCoreError> {
        self.repo
            .increment_user_ai_usage_monthly_count(user_id, period)
            .map_err(|e| ServerCoreError::Internal(e.to_string()))
    }

    async fn check_rate_limit(
        &self,
        _user_id: &str,
        _proxy_id: &str,
        _limit: u32,
    ) -> Result<bool, ServerCoreError> {
        // Rate limiting is handled by the in-memory RateLimiter in the handler
        Ok(true)
    }
}
